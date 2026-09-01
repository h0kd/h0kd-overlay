/**
 * Cliente de Twitch: OAuth Authorization Code Flow (cliente confidencial) y las
 * llamadas a Helix que necesita el Worker.
 *
 * El agente local usa Device Code Flow con su propio Client ID público; esto es
 * la otra mitad, la web, donde sí hay un client secret y vive solo acá.
 */

import type { Env } from './env.ts';

const AUTHORIZE = 'https://id.twitch.tv/oauth2/authorize';
const TOKEN = 'https://id.twitch.tv/oauth2/token';
const HELIX = 'https://api.twitch.tv/helix';

export interface TwitchUser {
  id: string;
  login: string;
  display_name: string;
  profile_image_url: string;
}

export interface TokenPair {
  access_token: string;
  refresh_token: string;
  expires_in: number;
}

/**
 * Scopes por rol.
 *
 * Viewers y mods: NINGUNO. Solo se necesita saber quiénes son, y pedir permisos
 * que no se usan es la forma más rápida de que la gente no autorice.
 *
 * Broadcaster: `moderation:read` para listar los mods reales del canal en
 * /admin vía Helix `Get Moderators`. Las suscripciones de EventSub a
 * stream.online/offline no piden scope, pero sí un app access token.
 */
export const SCOPES = {
  viewer: '',
  broadcaster: 'moderation:read',
} as const;

/**
 * Twitch explica en el body POR QUÉ rechazó (secret equivocado, redirect_uri
 * que no coincide, code ya usado). Tragarse eso y reportar solo el status
 * convierte cada problema de configuración en una adivinanza.
 */
async function twitchError(step: string, res: Response): Promise<string> {
  const detail = await res.text().catch(() => '');
  return `Twitch rechazó el ${step} (${res.status}): ${detail.slice(0, 300)}`;
}

export function authorizeUrl(env: Env, state: string, scope: string): string {
  const p = new URLSearchParams({
    client_id: env.TWITCH_CLIENT_ID,
    redirect_uri: `${env.PUBLIC_ORIGIN}/auth/callback`,
    response_type: 'code',
    scope,
    state,
    // Sin esto, Twitch reusa la autorización previa y el usuario nunca ve qué
    // permisos está dando cuando cambian los scopes.
    force_verify: 'false',
  });
  return `${AUTHORIZE}?${p}`;
}

/**
 * Un secret vacío es un modo de falla real: `wrangler secret put` sin terminal
 * interactiva guarda una cadena vacía sin quejarse, y el nombre igual aparece
 * en `secret list`. Sin este chequeo el síntoma es un 400 de Twitch que parece
 * un problema de OAuth y no de configuración.
 */
function requireSecret(env: Env): void {
  if (!env.TWITCH_CLIENT_SECRET) {
    throw new Error(
      'TWITCH_CLIENT_SECRET está vacío o sin cargar. Cargalo desde el dashboard ' +
        'de Cloudflare o con `wrangler secret put` en una terminal real.',
    );
  }
}

export async function exchangeCode(env: Env, code: string): Promise<TokenPair> {
  requireSecret(env);
  const res = await fetch(TOKEN, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      client_id: env.TWITCH_CLIENT_ID,
      client_secret: env.TWITCH_CLIENT_SECRET,
      code,
      grant_type: 'authorization_code',
      redirect_uri: `${env.PUBLIC_ORIGIN}/auth/callback`,
    }),
  });
  if (!res.ok) throw new Error(await twitchError('canje del code', res));
  return (await res.json()) as TokenPair;
}

export async function refreshToken(env: Env, refresh: string): Promise<TokenPair> {
  requireSecret(env);
  const res = await fetch(TOKEN, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      client_id: env.TWITCH_CLIENT_ID,
      client_secret: env.TWITCH_CLIENT_SECRET,
      grant_type: 'refresh_token',
      refresh_token: refresh,
    }),
  });
  if (!res.ok) throw new Error(await twitchError('refresh del token', res));
  return (await res.json()) as TokenPair;
}

/** App access token (client credentials), para administrar EventSub. */
export async function appToken(env: Env): Promise<string> {
  requireSecret(env);
  const res = await fetch(TOKEN, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      client_id: env.TWITCH_CLIENT_ID,
      client_secret: env.TWITCH_CLIENT_SECRET,
      grant_type: 'client_credentials',
    }),
  });
  if (!res.ok) throw new Error(await twitchError('app token', res));
  return ((await res.json()) as { access_token: string }).access_token;
}

async function helix<T>(env: Env, path: string, token: string): Promise<T> {
  const res = await fetch(`${HELIX}${path}`, {
    headers: { 'Client-Id': env.TWITCH_CLIENT_ID, Authorization: `Bearer ${token}` },
  });
  if (res.status === 401) throw new UnauthorizedError();
  if (!res.ok) throw new Error(`Helix ${path} devolvió ${res.status}`);
  return (await res.json()) as T;
}

export class UnauthorizedError extends Error {
  constructor() {
    super('token de Twitch inválido o vencido');
    this.name = 'UnauthorizedError';
  }
}

/** El usuario dueño del token. */
export async function getSelf(env: Env, token: string): Promise<TwitchUser> {
  const body = await helix<{ data: TwitchUser[] }>(env, '/users', token);
  const user = body.data[0];
  if (!user) throw new Error('Twitch /users devolvió vacío');
  return user;
}

export interface Moderator {
  user_id: string;
  user_login: string;
  user_name: string;
}

/**
 * Lista los mods reales del canal. Requiere el user token del broadcaster con
 * `moderation:read`; pagina hasta agotar (100 por página).
 */
export async function getModerators(
  env: Env,
  broadcasterId: string,
  token: string,
): Promise<Moderator[]> {
  const out: Moderator[] = [];
  let cursor: string | undefined;
  // Tope duro: 20 páginas = 2000 mods. Un canal con más que eso es un caso que
  // no existe, y un bucle sin techo contra una API externa sí es un problema.
  for (let page = 0; page < 20; page++) {
    const q = new URLSearchParams({ broadcaster_id: broadcasterId, first: '100' });
    if (cursor) q.set('after', cursor);
    const body = await helix<{ data: Moderator[]; pagination?: { cursor?: string } }>(
      env,
      `/moderation/moderators?${q}`,
      token,
    );
    out.push(...body.data);
    cursor = body.pagination?.cursor;
    if (!cursor) break;
  }
  return out;
}

/**
 * Fotos de perfil por login, en una sola llamada.
 *
 * Helix acepta hasta 100 `login` por request, así que una lista de mods o de
 * viewers entra entera y no hay que pedir de a uno. Si la llamada falla se
 * devuelve un mapa vacío a propósito: quedarse sin fotos es cosmético, y una
 * página de administración que no carga porque Twitch tosió, no.
 */
export async function getUserPics(
  env: Env,
  logins: string[],
  token: string,
): Promise<Map<string, string>> {
  const out = new Map<string, string>();
  const unicos = [...new Set(logins.map((l) => l.toLowerCase()).filter(Boolean))];
  for (let i = 0; i < unicos.length; i += 100) {
    const q = new URLSearchParams();
    for (const l of unicos.slice(i, i + 100)) q.append('login', l);
    try {
      const body = await helix<{ data: TwitchUser[] }>(env, `/users?${q}`, token);
      for (const u of body.data) out.set(u.login.toLowerCase(), u.profile_image_url);
    } catch {
      /* sin fotos se sigue igual */
    }
  }
  return out;
}

// ── EventSub (transporte webhook) ────────────────────────────────────────────

const STREAM_EVENTS = ['stream.online', 'stream.offline'] as const;

/**
 * Suscribe el canal a stream.online/offline por webhook. Idempotente en la
 * práctica: Twitch responde 409 si la suscripción ya existe, y eso no es error.
 */
export async function subscribeStreamEvents(env: Env, broadcasterId: string): Promise<void> {
  const token = await appToken(env);
  const callback = `${env.PUBLIC_ORIGIN}/eventsub`;

  // Si el Worker cambió de dominio, las suscripciones viejas siguen apuntando
  // al callback anterior y Twitch las sigue considerando "existentes": el POST
  // de abajo daría 409 y el canal quedaría escuchando en una URL muerta. Se
  // borran las que no apunten al callback actual antes de crear las nuevas.
  await dropStaleSubscriptions(env, token, broadcasterId, callback);

  for (const type of STREAM_EVENTS) {
    const res = await fetch(`${HELIX}/eventsub/subscriptions`, {
      method: 'POST',
      headers: {
        'Client-Id': env.TWITCH_CLIENT_ID,
        Authorization: `Bearer ${token}`,
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        type,
        version: '1',
        condition: { broadcaster_user_id: broadcasterId },
        transport: {
          method: 'webhook',
          callback,
          secret: env.EVENTSUB_SECRET,
        },
      }),
    });
    if (res.status === 409) continue; // ya existía
    if (!res.ok) {
      throw new Error(`No se pudo suscribir ${type} (${res.status}): ${await res.text()}`);
    }
  }
}

interface EventSubRow {
  id: string;
  type: string;
  condition: { broadcaster_user_id?: string };
  transport: { method: string; callback?: string };
}

/**
 * Borra las suscripciones de stream.online/offline de este canal que apunten a
 * otro callback. Best effort: si listar o borrar falla, se sigue igual y el
 * POST de alta dirá lo suyo.
 */
async function dropStaleSubscriptions(
  env: Env,
  token: string,
  broadcasterId: string,
  callback: string,
): Promise<void> {
  const headers = { 'Client-Id': env.TWITCH_CLIENT_ID, Authorization: `Bearer ${token}` };
  let rows: EventSubRow[];
  try {
    const res = await fetch(
      `${HELIX}/eventsub/subscriptions?user_id=${encodeURIComponent(broadcasterId)}`,
      { headers },
    );
    if (!res.ok) return;
    rows = ((await res.json()) as { data?: EventSubRow[] }).data ?? [];
  } catch {
    return;
  }
  for (const row of rows) {
    const ours = (STREAM_EVENTS as readonly string[]).includes(row.type)
      && row.transport.method === 'webhook'
      && row.condition.broadcaster_user_id === broadcasterId;
    if (!ours || row.transport.callback === callback) continue;
    console.log(`[eventsub] borro ${row.type} que apuntaba a ${row.transport.callback}`);
    try {
      await fetch(`${HELIX}/eventsub/subscriptions?id=${encodeURIComponent(row.id)}`, {
        method: 'DELETE',
        headers,
      });
    } catch {
      // Se reintenta en la próxima visita a /admin.
    }
  }
}
