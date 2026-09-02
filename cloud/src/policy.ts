/**
 * Allowlist de dominios y política de envío.
 *
 * Esta es la primera defensa del sistema: se ejecuta en el Worker ANTES de que
 * nadie le pase una URL a yt-dlp. Sin allowlist, yt-dlp cae en su extractor
 * genérico y se convierte en un "traeme esta URL" a discreción del que envía
 * (SSRF, IPs internas, archivos gigantes). El agente igual revalida.
 */

import type { Platform } from './protocol.ts';

/** host exacto o sufijo `.host` -> plataforma. */
const ALLOWED: Array<{ host: string; platform: Platform }> = [
  { host: 'instagram.com', platform: 'instagram' },
  // vm./vt.tiktok.com entran por la regla de sufijo: son los links cortos que
  // genera la propia app y redirigen dentro de TikTok.
  { host: 'tiktok.com', platform: 'tiktok' },
  { host: 'twitch.tv', platform: 'twitch' },
  { host: 'clips.twitch.tv', platform: 'twitch' },
  { host: 'youtube.com', platform: 'youtube' },
  { host: 'youtu.be', platform: 'youtube' },
  // kappa.lol es un host de archivos (el uploader de Chatterino), no una red:
  // sirve el mp4 directo y yt-dlp lo baja con su extractor genérico. Solo el
  // host pelado: sus subdominios (w.kappa.lol) son otra cosa.
  { host: 'kappa.lol', platform: 'kappa' },
];

export type UrlCheck =
  | { ok: true; platform: Platform; url: string }
  | { ok: false; reason: string };

/**
 * Valida una URL enviada por un viewer y la normaliza.
 *
 * Normalizar importa tanto como validar: se descartan query y fragmento, que
 * es donde viven los parámetros de tracking y, peor, los intentos de colar
 * cosas raras al extractor.
 */
export function checkUrl(raw: string): UrlCheck {
  const trimmed = raw.trim();
  if (!trimmed) return { ok: false, reason: 'Pegá un link.' };
  if (trimmed.length > 2048) return { ok: false, reason: 'El link es demasiado largo.' };

  let u: URL;
  try {
    u = new URL(trimmed);
  } catch {
    return { ok: false, reason: 'Eso no parece un link válido.' };
  }

  // Solo https. http:// abre la puerta a redirects y a hosts internos.
  if (u.protocol !== 'https:') {
    return { ok: false, reason: 'El link tiene que empezar con https://' };
  }
  if (u.username || u.password) {
    return { ok: false, reason: 'El link no puede llevar usuario ni contraseña.' };
  }
  if (u.port) {
    return { ok: false, reason: 'El link no puede especificar un puerto.' };
  }

  const host = u.hostname.toLowerCase().replace(/^www\./, '');
  const match = ALLOWED.find((a) => host === a.host || host.endsWith('.' + a.host));
  if (!match) {
    return {
      ok: false,
      reason: 'Por ahora solo se aceptan links de TikTok, Instagram, YouTube, Twitch y kappa.lol.',
    };
  }

  // kappa.lol: un archivo por link, `kappa.lol/<id>`. La extensión y lo que
  // venga después del id son opcionales y el servidor los ignora, así que se
  // normaliza al id pelado. El index (`/`) y las páginas del sitio no pasan.
  if (match.platform === 'kappa') {
    if (host !== 'kappa.lol') {
      return { ok: false, reason: 'De kappa.lol solo se aceptan links a un archivo.' };
    }
    const m = /^\/([A-Za-z0-9]{3,32})(?:\.[A-Za-z0-9]{1,8})?(?:\/.*)?$/.exec(u.pathname);
    if (!m || m[1] === 'uploaders' || m[1] === 'delete' || m[1] === 'api') {
      return { ok: false, reason: 'De kappa.lol solo se aceptan links a un archivo (kappa.lol/abc123).' };
    }
    return { ok: true, platform: 'kappa', url: `https://kappa.lol/${m[1]}` };
  }

  // Instagram: SOLO reels. Los posts (/p/) se aceptaban por si eran de video,
  // y en el primer stream real la mayoría fueron fotos: el agente los leía,
  // yt-dlp decía "There is no video in this post" y el viewer veía un fallo
  // críptico minutos después. Mejor decírselo al enviar, con el link correcto.
  if (match.platform === 'instagram') {
    const okPath = /^\/(reel|reels)\/[A-Za-z0-9_-]+\/?$/.test(u.pathname);
    if (!okPath) {
      return {
        ok: false,
        reason: 'De Instagram solo se aceptan Reels (instagram.com/reel/...). Los posts y fotos no.',
      };
    }
  }

  // TikTok tiene dos formas de link y hay que aceptar las dos, porque la app
  // comparte una y el navegador la otra. El link corto (vm./vt.) no dice nada
  // en el path, así que se acepta por el host. Lo que no pasa nunca es el
  // perfil pelado: para yt-dlp eso es una playlist entera, no un video.
  if (match.platform === 'tiktok') {
    const corto = host === 'vm.tiktok.com' || host === 'vt.tiktok.com';
    const okPath = corto
      ? /^\/[A-Za-z0-9]+\/?$/.test(u.pathname)
      : /^\/(@[A-Za-z0-9_.]+\/video|t)\/[A-Za-z0-9]+\/?$/.test(u.pathname);
    if (!okPath) {
      return { ok: false, reason: 'De TikTok solo se aceptan videos, no perfiles.' };
    }
    // yt-dlp EXIGE el `www.` en los links largos de TikTok: sin él ni siquiera
    // llega a su extractor y contesta "Unsupported URL". Sacar el www es lo
    // correcto para normalizar cualquier otro dominio, y acá rompe todo.
    return {
      ok: true,
      platform: 'tiktok',
      url: `https://${corto ? host : 'www.tiktok.com'}${u.pathname}`,
    };
  }

  // YouTube es la excepción a tirar la query: en /watch el id del video vive
  // ahí. Se conserva `v` y solo `v` — el resto son tracking, playlist y tiempo,
  // que es justo lo que hay que dejar afuera. Sin esto, el link que pega
  // cualquiera (youtube.com/watch?v=...) llegaba al agente como /watch pelado.
  if (match.platform === 'youtube' && u.pathname === '/watch') {
    const v = u.searchParams.get('v') ?? '';
    if (!/^[A-Za-z0-9_-]{6,20}$/.test(v)) {
      return { ok: false, reason: 'Ese link de YouTube no apunta a un video.' };
    }
    return { ok: true, platform: 'youtube', url: `https://${host}/watch?v=${v}` };
  }

  return { ok: true, platform: match.platform, url: `https://${host}${u.pathname}` };
}

/** Dominios permitidos, para mostrarlos en la página de envío. */
export function allowedHosts(): string[] {
  return [...new Set(ALLOWED.map((a) => a.host))];
}

// ── Política de envío ────────────────────────────────────────────────────────

export interface SubmissionPolicy {
  submissions_open: boolean;
  /** Lo último que dijo EventSub del stream. Solo cambia el mensaje. */
  stream_online: boolean;
  cooldown_seconds: number;
  max_pending_per_user: number;
}

export interface SubmitterState {
  /** epoch ms del último envío de este usuario, o null. */
  last_submit_at: number | null;
  /** cuántos ítems suyos siguen en juego (submitted..ready). */
  pending_count: number;
}

export type PolicyCheck = { ok: true } | { ok: false; reason: string };

export function checkPolicy(
  policy: SubmissionPolicy,
  state: SubmitterState,
  now: number,
): PolicyCheck {
  if (!policy.submissions_open) {
    // Cerrado puede ser porque el stream terminó o porque el streamer los
    // pausó a mano en pleno directo. Decir "offline" en el segundo caso hizo
    // que los viewers le avisaran al streamer que Twitch lo había tirado.
    return {
      ok: false,
      reason: policy.stream_online
        ? 'Los envíos están pausados por ahora: el streamer los cerró.'
        : 'Los envíos están cerrados (el stream está offline).',
    };
  }
  if (state.pending_count >= policy.max_pending_per_user) {
    return {
      ok: false,
      reason: `Ya tenés ${state.pending_count} pedido(s) en cola. Esperá a que se reproduzcan.`,
    };
  }
  if (state.last_submit_at !== null) {
    const waited = (now - state.last_submit_at) / 1000;
    if (waited < policy.cooldown_seconds) {
      const left = Math.ceil(policy.cooldown_seconds - waited);
      return { ok: false, reason: `Esperá ${left} segundo(s) antes de mandar otro.` };
    }
  }
  return { ok: true };
}
