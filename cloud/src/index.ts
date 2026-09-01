/**
 * Worker: páginas, API, OAuth, emparejamiento del agente y webhook de EventSub.
 *
 * Reglas que valen para todos los handlers de acá:
 *   - La identidad sale de la sesión firmada. Nunca del body ni de la query.
 *   - El `channel_id` se resuelve contra D1 y se pasa explícito a cada query.
 *   - Antes de tocar nada de yt-dlp, la URL pasa por la allowlist.
 */

import { Hono, type Context } from 'hono';
import { getSession, setSession, clearSession, beginOAuth, consumeOAuth, roleFor, safeRedirect } from './auth.ts';
import { decryptToken, encryptToken, pairingCode, randomToken, sha256Hex } from './crypto.ts';
import { PAIRING_TTL_SECONDS, type Env } from './env.ts';
import { handleEventSub } from './eventsub.ts';
import { adminPage, modPage, submitPage } from './pages.ts';
import { allowedHosts, checkPolicy, checkUrl } from './policy.ts';
import * as q from './queue.ts';
import * as tw from './twitch.ts';

export { ChannelHub } from './do.ts';

type App = { Bindings: Env };
const app = new Hono<App>();

const STATUS_LABEL: Record<string, string> = {
  submitted: 'leyendo el video…',
  pending_review: 'esperando que un mod lo revise',
  approved: 'aprobado',
  downloading: 'descargando',
  ready: 'en cola para reproducirse',
  playing: 'reproduciéndose',
  played: 'ya se reprodujo',
  rejected: 'rechazado por un mod',
  rejected_auto: 'rechazado automáticamente',
  failed: 'falló',
  cleared: 'cancelado (terminó el stream)',
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Body JSON tolerante. Un body ausente, vacío o roto se trata como "sin campos"
 * y cada handler decide qué falta: así un POST malformado da un 400 con motivo
 * en vez de un 500.
 */
async function readJson<T extends object>(c: Context<App>): Promise<Partial<T>> {
  return (await c.req.json().catch(() => ({}))) as Partial<T>;
}

/** Resuelve `?ch=` (login o id) contra los canales dados de alta. */
async function resolveChannel(env: Env, ch: string | undefined) {
  const raw = (ch ?? '').trim().toLowerCase();
  if (!raw) return null;
  return (await q.channelByLogin(env.DB, raw)) ?? (await q.channelById(env.DB, raw));
}

/** Empuja un aviso al hub del canal. Que falle no puede voltear el request. */
async function notifyHub(
  env: Env,
  channel: { channel_id: string; twitch_login: string },
  path: string,
  body?: unknown,
): Promise<void> {
  try {
    const stub = env.CHANNEL_HUB.get(env.CHANNEL_HUB.idFromName(channel.channel_id));
    await stub.fetch(`https://hub${path}`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'x-channel-id': channel.channel_id,
        'x-channel-login': channel.twitch_login,
      },
      body: JSON.stringify(body ?? {}),
    });
  } catch (err) {
    console.error('[hub] no se pudo notificar', path, err);
  }
}

/** Token de Helix del broadcaster, renovándolo si venció. */
async function broadcasterToken(env: Env, channelId: string): Promise<string | null> {
  const row = await env.DB.prepare('SELECT * FROM broadcaster_tokens WHERE channel_id = ?')
    .bind(channelId)
    .first<{ access_token: string; refresh_token: string; expires_at: number }>();
  if (!row) return null;

  if (row.expires_at > Date.now() + 60_000) {
    return decryptToken(env.TOKEN_ENC_KEY, row.access_token);
  }
  const refresh = await decryptToken(env.TOKEN_ENC_KEY, row.refresh_token);
  if (!refresh) return null;
  try {
    const fresh = await tw.refreshToken(env, refresh);
    await storeBroadcasterToken(env, channelId, fresh);
    return fresh.access_token;
  } catch {
    return null;
  }
}

async function storeBroadcasterToken(env: Env, channelId: string, t: tw.TokenPair): Promise<void> {
  await env.DB.prepare(
    `INSERT INTO broadcaster_tokens (channel_id, access_token, refresh_token, expires_at)
     VALUES (?, ?, ?, ?)
     ON CONFLICT(channel_id) DO UPDATE SET
       access_token = excluded.access_token,
       refresh_token = excluded.refresh_token,
       expires_at = excluded.expires_at`,
  )
    .bind(
      channelId,
      await encryptToken(env.TOKEN_ENC_KEY, t.access_token),
      await encryptToken(env.TOKEN_ENC_KEY, t.refresh_token),
      Date.now() + t.expires_in * 1000,
    )
    .run();
}

/** Beta cerrado: si ALLOWED_CHANNELS está seteada, solo esos logins entran. */
function channelAllowed(env: Env, login: string): boolean {
  const list = (env.ALLOWED_CHANNELS ?? '').trim();
  if (!list) return true;
  return list
    .toLowerCase()
    .split(',')
    .map((s) => s.trim())
    .includes(login.toLowerCase());
}

// ── Páginas ──────────────────────────────────────────────────────────────────

app.get('/', (c) => c.redirect('/admin'));
app.get('/submit', (c) => c.html(submitPage()));
app.get('/mod', (c) => c.html(modPage()));
app.get('/admin', (c) => c.html(adminPage()));

// ── Diagnóstico ──────────────────────────────────────────────────────────────

/**
 * Chequeo de configuración. Devuelve SOLO booleanos: nunca un valor, ni un
 * prefijo, ni un largo.
 *
 * Existe porque `wrangler secret list` y el dashboard muestran el NOMBRE de un
 * secret aunque su valor sea una cadena vacía, y `wrangler secret put` sin
 * terminal interactiva guarda vacío sin quejarse. El resultado es un despliegue
 * que parece completo y falla más tarde con un error que apunta a otro lado.
 */
app.get('/health', async (c) => {
  const env = c.env;

  // La clave de cifrado no alcanza con que exista: tiene que ser importable
  // como AES-GCM de 32 bytes, o los tokens del broadcaster no se pueden guardar.
  let tokenKeyUsable = false;
  try {
    await encryptToken(env.TOKEN_ENC_KEY ?? '', 'prueba');
    tokenKeyUsable = true;
  } catch {
    tokenKeyUsable = false;
  }

  let dbOk = false;
  try {
    await env.DB.prepare('SELECT 1').first();
    dbOk = true;
  } catch {
    dbOk = false;
  }

  const checks = {
    twitch_client_id: !!env.TWITCH_CLIENT_ID && env.TWITCH_CLIENT_ID !== 'REEMPLAZAR',
    twitch_client_secret: !!env.TWITCH_CLIENT_SECRET,
    session_secret: !!env.SESSION_SECRET,
    token_enc_key_usable: tokenKeyUsable,
    eventsub_secret: !!env.EVENTSUB_SECRET,
    public_origin: env.PUBLIC_ORIGIN === new URL(c.req.url).origin,
    allowed_channels_set: !!(env.ALLOWED_CHANNELS ?? '').trim(),
    database: dbOk,
  };
  const ok = Object.values(checks).every(Boolean);
  return c.json({ ok, checks }, ok ? 200 : 503);
});

// ── OAuth ────────────────────────────────────────────────────────────────────

app.get('/auth/login', async (c) => {
  const to = safeRedirect(c.req.query('to') ?? '/');
  // /admin necesita moderation:read para listar los mods reales del canal.
  // Viewers y mods no necesitan ningún scope: solo identidad.
  const scope = to.startsWith('/admin') ? tw.SCOPES.broadcaster : tw.SCOPES.viewer;
  const state = await beginOAuth(c, to);
  return c.redirect(tw.authorizeUrl(c.env, state, scope));
});

app.get('/auth/callback', async (c) => {
  const code = c.req.query('code');
  const state = c.req.query('state');
  if (!code || !state) return c.text('Faltan parámetros de OAuth.', 400);

  const pending = await consumeOAuth(c, state);
  if (!pending) return c.text('El login expiró o no coincide. Probá de nuevo.', 400);

  let tokens: tw.TokenPair;
  let user: tw.TwitchUser;
  try {
    tokens = await tw.exchangeCode(c.env, code);
    user = await tw.getSelf(c.env, tokens.access_token);
  } catch (err) {
    console.error('[oauth]', err);
    return c.text('Twitch rechazó el login.', 502);
  }

  await setSession(c, user);

  // Login de broadcaster: dar de alta el canal, guardar el token para Helix y
  // asegurar las suscripciones de EventSub.
  if (pending.to.startsWith('/admin')) {
    if (!channelAllowed(c.env, user.login)) {
      return c.text('Este canal todavía no está habilitado en el beta.', 403);
    }
    await q.ensureChannel(c.env.DB, user.id, user.login.toLowerCase());
    await storeBroadcasterToken(c.env, user.id, tokens);
    try {
      await tw.subscribeStreamEvents(c.env, user.id);
    } catch (err) {
      // No es fatal: sin esto los envíos no se abren/cierran solos, pero todo
      // lo demás anda. Se reintenta en la próxima visita a /admin.
      console.error('[eventsub] no se pudo suscribir', err);
    }
  }

  return c.redirect(pending.to);
});

app.post('/auth/logout', (c) => {
  clearSession(c);
  return c.json({ ok: true });
});

// ── API pública ──────────────────────────────────────────────────────────────

app.get('/api/me', async (c) => {
  const session = await getSession(c);
  const chParam = c.req.query('ch');
  if (!chParam) {
    return c.json({ login: session?.login ?? null, name: session?.name ?? null, pic: session?.pic ?? null });
  }
  const channel = await resolveChannel(c.env, chParam);
  if (!channel) return c.json({ error: 'Ese canal no está dado de alta.' }, 404);

  const settings = await q.getSettings(c.env.DB, channel.channel_id);
  return c.json({
    login: session?.login ?? null,
    pic: session?.pic ?? null,
    role: await roleFor(c.env, session, channel.channel_id),
    channel_login: channel.twitch_login,
    submissions_open: settings.submissions_open,
    allowed_hosts: allowedHosts(),
    max_duration_seconds: settings.max_duration_seconds,
  });
});

app.post('/api/submit', async (c) => {
  const session = await getSession(c);
  if (!session) return c.json({ error: 'Iniciá sesión con Twitch.' }, 401);

  const body = await readJson<{ ch: string; url: string }>(c);
  const channel = await resolveChannel(c.env, body.ch);
  if (!channel) return c.json({ error: 'Ese canal no está dado de alta.' }, 404);

  const check = checkUrl(body.url ?? '');
  if (!check.ok) return c.json({ error: check.reason }, 400);

  const settings = await q.getSettings(c.env.DB, channel.channel_id);
  const state = await q.submitterState(c.env.DB, channel.channel_id, session.uid);
  const policy = checkPolicy(settings, state, Date.now());
  if (!policy.ok) return c.json({ error: policy.reason }, 429);

  await q.insertItem(c.env.DB, {
    id: crypto.randomUUID(),
    channel_id: channel.channel_id,
    submitter_twitch_id: session.uid,
    submitter_login: session.login,
    source_url: check.url,
    platform: check.platform,
  });
  await notifyHub(c.env, channel, '/internal/queue-changed');
  return c.json({ ok: true });
});

interface MineRow {
  id: string;
  source_url: string;
  platform: string;
  title: string | null;
  thumbnail_url: string | null;
  duration_seconds: number | null;
  status: string;
  error: string | null;
  decided_by: string | null;
  decided_at: number | null;
  decided_reason: string | null;
  created_at: number;
}

app.get('/api/mine', async (c) => {
  const session = await getSession(c);
  if (!session) return c.json({ error: 'Iniciá sesión con Twitch.' }, 401);
  const channel = await resolveChannel(c.env, c.req.query('ch'));
  if (!channel) return c.json({ error: 'Ese canal no está dado de alta.' }, 404);

  const res = await c.env.DB.prepare(
    `SELECT id, source_url, platform, title, thumbnail_url, duration_seconds,
            status, error, decided_by, decided_at, decided_reason, created_at
       FROM queue_items
      WHERE channel_id = ? AND submitter_twitch_id = ?
      ORDER BY created_at DESC LIMIT 10`,
  )
    .bind(channel.channel_id, session.uid)
    .all<MineRow>();

  // La posición en cola sale de mirar TODOS los pedidos en juego del canal, no
  // solo los de esta persona: "sos el cuarto" no significa nada si se cuenta
  // únicamente lo tuyo. Es una segunda consulta y no un JOIN porque la cola en
  // juego son unos pocos ítems y así el orden queda explícito acá.
  const enJuego = await c.env.DB.prepare(
    `SELECT id FROM queue_items
      WHERE channel_id = ? AND status IN ('submitted','pending_review','approved','downloading','ready','playing')
      ORDER BY created_at ASC`,
  )
    .bind(channel.channel_id)
    .all<{ id: string }>();
  const posicion = new Map((enJuego.results ?? []).map((r, i) => [r.id, i + 1]));

  return c.json({
    items: (res.results ?? []).map((r) => ({
      ...r,
      status_label: STATUS_LABEL[r.status] ?? r.status,
      position: posicion.get(r.id) ?? null,
    })),
  });
});

// ── API de moderación ────────────────────────────────────────────────────────

app.post('/api/decide', async (c) => {
  const session = await getSession(c);
  const body = await readJson<{ ch: string; item_id: string; approved: boolean; reason?: string }>(c);
  const channel = await resolveChannel(c.env, body.ch);
  if (!channel) return c.json({ error: 'Ese canal no está dado de alta.' }, 404);

  const role = await roleFor(c.env, session, channel.channel_id);
  if (role !== 'mod' && role !== 'broadcaster') {
    return c.json({ error: 'No tenés permiso de moderación en este canal.' }, 403);
  }
  if (!body.item_id) return c.json({ error: 'Falta el item.' }, 400);
  // El motivo es texto libre de un humano: se recorta y se acota acá, que es
  // donde entra al sistema. Vacío vale: dar el motivo es opcional.
  const motivo = typeof body.reason === 'string' ? body.reason.trim().slice(0, 300) : '';

  await notifyHub(c.env, channel, '/internal/decision', {
    item_id: body.item_id,
    approved: body.approved === true,
    by: session!.login,
    reason: motivo || null,
  });
  return c.json({ ok: true });
});

interface HistoryRow {
  id: string;
  source_url: string;
  platform: string;
  title: string | null;
  thumbnail_url: string | null;
  duration_seconds: number | null;
  status: string;
  error: string | null;
  submitter_login: string;
  decided_by: string | null;
  decided_at: number | null;
  decided_reason: string | null;
  created_at: number;
}

const HISTORY_COLS = `id, source_url, platform, title, thumbnail_url, duration_seconds,
                      status, error, submitter_login, decided_by, decided_at,
                      decided_reason, created_at`;

/**
 * Historial de lo que ya se decidió.
 *
 * Con `?mine=1` devuelve solo lo que decidió quien pregunta —el "mi historial"
 * del panel de mods, para revisar lo propio sin ver el trabajo ajeno—; sin eso,
 * el historial completo del canal, que es del broadcaster.
 *
 * El filtro por mod se hace por `decided_by`, que guarda el login. Es lo mismo
 * que muestra el panel, así que un mod ve exactamente lo que firmó.
 */
app.get('/api/history', async (c) => {
  const session = await getSession(c);
  const channel = await resolveChannel(c.env, c.req.query('ch'));
  if (!channel) return c.json({ error: 'Ese canal no está dado de alta.' }, 404);

  const role = await roleFor(c.env, session, channel.channel_id);
  if (role !== 'mod' && role !== 'broadcaster') {
    return c.json({ error: 'No tenés permiso de moderación en este canal.' }, 403);
  }

  const soloMias = c.req.query('mine') === '1';
  if (soloMias && !session) return c.json({ error: 'Iniciá sesión con Twitch.' }, 401);

  // Terminados: lo que ya no está en juego. `cleared` entra porque para el mod
  // "se limpió al terminar el stream" también es un final que quiere ver.
  const estados = ['played', 'rejected', 'failed', 'cleared'];
  const marcas = estados.map(() => '?').join(',');
  const sql = soloMias
    ? `SELECT ${HISTORY_COLS} FROM queue_items
        WHERE channel_id = ? AND decided_by = ? AND status IN (${marcas})
        ORDER BY COALESCE(decided_at, created_at) DESC LIMIT 40`
    : `SELECT ${HISTORY_COLS} FROM queue_items
        WHERE channel_id = ? AND status IN (${marcas})
        ORDER BY COALESCE(decided_at, created_at) DESC LIMIT 40`;
  const args = soloMias
    ? [channel.channel_id, session!.login, ...estados]
    : [channel.channel_id, ...estados];

  const res = await c.env.DB.prepare(sql).bind(...args).all<HistoryRow>();
  return c.json({ items: res.results ?? [] });
});

// ── API de admin (solo el broadcaster, sobre su propio canal) ────────────────

/** Exige sesión de broadcaster y devuelve su canal. */
async function requireBroadcaster(c: Context<App>) {
  const session = await getSession(c);
  if (!session) return { error: c.json({ error: 'Iniciá sesión con Twitch.' }, 401) };
  const channel = await q.channelById(c.env.DB, session.uid);
  if (!channel) {
    return { error: c.json({ error: 'Entrá a /admin con la cuenta del canal para darlo de alta.' }, 403) };
  }
  return { session, channel };
}

app.get('/api/admin/overview', async (c) => {
  const auth = await requireBroadcaster(c);
  if (auth.error) return auth.error;
  const { channel } = auth;

  const token = await broadcasterToken(c.env, channel.channel_id);
  if (!token) return c.json({ error: 'Volvé a entrar con Twitch para renovar el permiso.' }, 401);

  let mods: tw.Moderator[];
  try {
    mods = await tw.getModerators(c.env, channel.channel_id, token);
  } catch (err) {
    if (err instanceof tw.UnauthorizedError) {
      return c.json({ error: 'Volvé a entrar con Twitch para renovar el permiso.' }, 401);
    }
    throw err;
  }

  // Los autorizados que ya no son mods en Twitch se siguen mostrando, marcados:
  // hacen falta para poder revocarles el acceso.
  const authorized = await q.listAuthorizedMods(c.env.DB, channel.channel_id);
  const byId = new Map(mods.map((m) => [m.user_id, m]));
  const rows = [
    ...mods.map((m) => ({
      user_id: m.user_id,
      user_login: m.user_login,
      user_name: m.user_name,
      still_mod: true,
      authorized: authorized.some((a) => a.twitch_user_id === m.user_id),
    })),
    ...authorized
      .filter((a) => !byId.has(a.twitch_user_id))
      .map((a) => ({
        user_id: a.twitch_user_id,
        user_login: a.twitch_login,
        user_name: a.twitch_login,
        still_mod: false,
        authorized: true,
      })),
  ];

  // Actividad por mod. `decided_by` guarda el login del que decidió, que es
  // exactamente lo mismo que muestra el panel, así que empareja derecho.
  const actividad = await c.env.DB.prepare(
    `SELECT LOWER(decided_by) AS login,
            SUM(CASE WHEN status IN ${APROBADOS} THEN 1 ELSE 0 END) AS aprobados,
            SUM(CASE WHEN status = 'rejected' THEN 1 ELSE 0 END)    AS rechazados,
            MAX(decided_at)                                          AS ultima
       FROM queue_items
      WHERE channel_id = ? AND decided_by IS NOT NULL AND decided_by <> ''
      GROUP BY LOWER(decided_by)`,
  )
    .bind(channel.channel_id)
    .all<{ login: string; aprobados: number; rechazados: number; ultima: number | null }>();
  const porMod = new Map((actividad.results ?? []).map((r) => [r.login, r]));

  // El broadcaster va en la tabla aunque Twitch no lo liste como mod de su
  // propio canal: es quien más decide, y sin esta fila la tabla dice que nadie
  // decidió nunca nada mientras la cola muestra lo contrario.
  rows.unshift({
    user_id: channel.channel_id,
    user_login: channel.twitch_login,
    user_name: channel.twitch_login,
    still_mod: true,
    authorized: true,
  });

  const fotos = await tw.getUserPics(c.env, rows.map((r) => r.user_login), token);

  const conDatos = rows.map((r) => {
    const login = r.user_login.toLowerCase();
    const act = porMod.get(login);
    return {
      ...r,
      es_dueno: r.user_id === channel.channel_id,
      pic: fotos.get(login) ?? null,
      es_bot: BOTS.has(login),
      aprobados: act?.aprobados ?? 0,
      rechazados: act?.rechazados ?? 0,
      ultima_accion: act?.ultima ?? null,
    };
  });

  return c.json({ mods: conDatos, settings: await q.getSettings(c.env.DB, channel.channel_id) });
});

app.post('/api/admin/mods', async (c) => {
  const auth = await requireBroadcaster(c);
  if (auth.error) return auth.error;
  const body = await readJson<{ user_id: string; login: string; authorized: boolean }>(c);
  if (!body.user_id || !body.login) return c.json({ error: 'Faltan datos del mod.' }, 400);

  await q.setModAuthorized(
    c.env.DB,
    auth.channel.channel_id,
    body.user_id,
    body.login.toLowerCase(),
    body.authorized === true,
  );
  return c.json({ ok: true });
});

/**
 * Cuentas de bot conocidas. No hay forma de preguntarle a Twitch si una cuenta
 * es un bot, así que esto es una lista a mano: cubre los sospechosos de siempre
 * y nada más. No se esconde nada, se marca — la UI decide si mostrarlos.
 */
const BOTS = new Set([
  'nightbot', 'streamelements', 'streamlabs', 'moobot', 'fossabot', 'wizebot',
  'phantombot', 'botisimo', 'coebot', 'deepbot', 'ankhbot', 'vivbot', 'sery_bot',
  'spanixbot', 'own3d', 'kofistreambot', 'soundalerts', 'pretzelrocks',
  'lumiastream', 'tangiabot', 'creatisbot', 'commanderroot', 'streamstickers',
]);

/** Estados que cuentan como "todavía en juego" en cualquier consulta. */
const EN_JUEGO = "('submitted','pending_review','approved','downloading','ready','playing')";
/** Estados que cuentan como aprobado: la decisión fue sí, pasara lo que pasara después. */
const APROBADOS = "('approved','downloading','ready','playing','played')";

/**
 * El resumen de arriba del panel.
 *
 * Todo sale de una sola consulta con sumas condicionales en vez de cinco
 * queries: son cinco números de la misma tabla y D1 cobra por viaje.
 */
app.get('/api/admin/stats', async (c) => {
  const auth = await requireBroadcaster(c);
  if (auth.error) return auth.error;

  const semana = Date.now() - 7 * 86400000;
  const dosSemanas = Date.now() - 14 * 86400000;
  const row = await c.env.DB.prepare(
    `SELECT
       SUM(CASE WHEN status IN ${EN_JUEGO} THEN 1 ELSE 0 END)                       AS en_cola,
       MIN(CASE WHEN status IN ${EN_JUEGO} THEN created_at END)                      AS mas_viejo,
       SUM(CASE WHEN status IN ${APROBADOS} AND decided_at >= ?1 THEN 1 ELSE 0 END)  AS aprobados,
       SUM(CASE WHEN status IN ${APROBADOS} AND decided_at >= ?2
                 AND decided_at < ?1 THEN 1 ELSE 0 END)                              AS aprobados_previos,
       SUM(CASE WHEN status = 'rejected' AND decided_at >= ?1 THEN 1 ELSE 0 END)     AS rechazados,
       SUM(CASE WHEN status = 'rejected' AND decided_at >= ?1
                 AND decided_reason IS NOT NULL AND decided_reason <> ''
                THEN 1 ELSE 0 END)                                                   AS con_motivo
     FROM queue_items WHERE channel_id = ?3`,
  )
    .bind(semana, dosSemanas, auth.channel.channel_id)
    .first<{
      en_cola: number | null; mas_viejo: number | null; aprobados: number | null;
      aprobados_previos: number | null; rechazados: number | null; con_motivo: number | null;
    }>();

  const aprobados = row?.aprobados ?? 0;
  const rechazados = row?.rechazados ?? 0;
  const revisados = aprobados + rechazados;
  return c.json({
    en_cola: row?.en_cola ?? 0,
    mas_viejo: row?.mas_viejo ?? null,
    aprobados,
    aprobados_previos: row?.aprobados_previos ?? 0,
    rechazados,
    con_motivo: row?.con_motivo ?? 0,
    revisados,
    // Sin nada revisado no hay tasa. Un 0% ahí sería mentira, no un dato.
    tasa: revisados ? Math.round((aprobados / revisados) * 100) : null,
  });
});

/** Actividad por viewer. Ordenada por quién más manda, que es lo que se mira. */
app.get('/api/admin/viewers', async (c) => {
  const auth = await requireBroadcaster(c);
  if (auth.error) return auth.error;

  const res = await c.env.DB.prepare(
    `SELECT submitter_login AS login,
            COUNT(*)                                                          AS enviados,
            SUM(CASE WHEN status IN ${APROBADOS} THEN 1 ELSE 0 END)           AS aprobados,
            SUM(CASE WHEN status = 'rejected' THEN 1 ELSE 0 END)              AS rechazados,
            SUM(CASE WHEN status IN ${EN_JUEGO} THEN 1 ELSE 0 END)            AS pendientes
       FROM queue_items WHERE channel_id = ?
      GROUP BY submitter_login
      ORDER BY enviados DESC, login ASC
      LIMIT 50`,
  )
    .bind(auth.channel.channel_id)
    .all<{ login: string; enviados: number; aprobados: number; rechazados: number; pendientes: number }>();

  const viewers = res.results ?? [];
  const token = await broadcasterToken(c.env, auth.channel.channel_id);
  const fotos = token
    ? await tw.getUserPics(c.env, viewers.map((v) => v.login), token)
    : new Map<string, string>();

  return c.json({
    viewers: viewers.map((v) => ({ ...v, pic: fotos.get(v.login.toLowerCase()) ?? null })),
  });
});

app.post('/api/admin/settings', async (c) => {
  const auth = await requireBroadcaster(c);
  if (auth.error) return auth.error;
  const patch = await readJson<Record<string, unknown>>(c);
  await q.updateSettings(c.env.DB, auth.channel.channel_id, patch);
  await notifyHub(c.env, auth.channel, '/internal/settings');
  return c.json({ ok: true });
});

app.post('/api/admin/pair', async (c) => {
  const auth = await requireBroadcaster(c);
  if (auth.error) return auth.error;

  const code = pairingCode();
  const now = Date.now();
  await c.env.DB.prepare(
    'INSERT INTO pairing_codes (code, channel_id, created_at, expires_at) VALUES (?, ?, ?, ?)',
  )
    .bind(code, auth.channel.channel_id, now, now + PAIRING_TTL_SECONDS * 1000)
    .run();
  return c.json({ code, expires_in: PAIRING_TTL_SECONDS });
});

// ── Agente ───────────────────────────────────────────────────────────────────

app.post('/agent/pair', async (c) => {
  const body = await readJson<{ code: string }>(c);
  const code = (body.code ?? '').trim().toUpperCase();
  if (!code) return c.json({ error: 'Falta el código.' }, 400);

  const row = await c.env.DB.prepare('SELECT * FROM pairing_codes WHERE code = ?')
    .bind(code)
    .first<{ code: string; channel_id: string; expires_at: number; used_at: number | null }>();
  if (!row || row.used_at !== null || row.expires_at < Date.now()) {
    return c.json({ error: 'Código inválido o vencido.' }, 400);
  }

  const token = randomToken();
  const now = Date.now();
  await c.env.DB.batch([
    c.env.DB.prepare('UPDATE pairing_codes SET used_at = ? WHERE code = ?').bind(now, code),
    // Se guarda el hash: la base nunca contiene un token utilizable tal cual.
    c.env.DB.prepare(
      'INSERT INTO agent_tokens (token_hash, channel_id, created_at) VALUES (?, ?, ?)',
    ).bind(await sha256Hex(token), row.channel_id, now),
    c.env.DB.prepare('UPDATE channels SET agent_paired_at = ? WHERE channel_id = ?').bind(
      now,
      row.channel_id,
    ),
  ]);

  const channel = await q.channelById(c.env.DB, row.channel_id);
  return c.json({
    agent_token: token,
    channel_id: row.channel_id,
    channel_login: channel?.twitch_login ?? '',
    worker_host: new URL(c.env.PUBLIC_ORIGIN).host,
  });
});

app.get('/agent/ws', async (c) => {
  const auth = c.req.header('Authorization') ?? '';
  const token = auth.startsWith('Bearer ') ? auth.slice(7) : '';
  if (!token) return c.text('falta el token', 401);

  const row = await c.env.DB.prepare(
    'SELECT channel_id, revoked_at FROM agent_tokens WHERE token_hash = ?',
  )
    .bind(await sha256Hex(token))
    .first<{ channel_id: string; revoked_at: number | null }>();
  if (!row || row.revoked_at !== null) return c.text('token invalido o revocado', 401);

  const channel = await q.channelById(c.env.DB, row.channel_id);
  if (!channel) return c.text('canal desconocido', 404);

  await c.env.DB.prepare('UPDATE agent_tokens SET last_seen_at = ? WHERE token_hash = ?')
    .bind(Date.now(), await sha256Hex(token))
    .run();

  return forwardToHub(c.env, channel, c.req.raw, '/ws/agent');
});

app.get('/mod/ws', async (c) => {
  const session = await getSession(c);
  const channel = await resolveChannel(c.env, c.req.query('ch'));
  if (!channel) return c.text('canal desconocido', 404);
  const role = await roleFor(c.env, session, channel.channel_id);
  if (role !== 'mod' && role !== 'broadcaster') return c.text('sin permiso', 403);
  return forwardToHub(c.env, channel, c.req.raw, '/ws/mod');
});

/** Reenvía un upgrade de WebSocket al hub del canal, con su identidad. */
function forwardToHub(
  env: Env,
  channel: { channel_id: string; twitch_login: string },
  request: Request,
  path: string,
): Promise<Response> {
  const stub = env.CHANNEL_HUB.get(env.CHANNEL_HUB.idFromName(channel.channel_id));
  const headers = new Headers(request.headers);
  headers.set('x-channel-id', channel.channel_id);
  headers.set('x-channel-login', channel.twitch_login);
  return stub.fetch(`https://hub${path}`, { headers });
}

// ── EventSub ─────────────────────────────────────────────────────────────────

app.post('/eventsub', async (c) => {
  const raw = await c.req.text();
  const result = await handleEventSub(c.env, c.req.raw.headers, raw);

  switch (result.kind) {
    case 'invalid':
      console.warn('[eventsub] rechazado:', result.reason);
      return c.text('firma invalida', 403);

    case 'challenge':
      return c.text(result.challenge, 200, { 'content-type': 'text/plain' });

    case 'notification': {
      const channel = await q.channelById(c.env.DB, result.broadcasterId);
      if (!channel) return noContent();
      if (result.type === 'stream.online' || result.type === 'stream.offline') {
        await notifyHub(c.env, channel, '/internal/stream', {
          online: result.type === 'stream.online',
        });
      }
      return noContent();
    }

    case 'revocation':
      console.warn('[eventsub] Twitch revocó', result.type, 'de', result.broadcasterId);
      return noContent();

    default:
      return noContent();
  }
});

/** 204 explícito: EventSub solo necesita el ACK, sin cuerpo. */
function noContent(): Response {
  return new Response(null, { status: 204 });
}

app.onError((err, c) => {
  console.error('[worker]', err);
  return c.json({ error: 'Error interno.' }, 500);
});

export default app;
