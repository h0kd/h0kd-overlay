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
  { host: 'twitch.tv', platform: 'twitch' },
  { host: 'clips.twitch.tv', platform: 'twitch' },
  { host: 'youtube.com', platform: 'youtube' },
  { host: 'youtu.be', platform: 'youtube' },
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
      reason: 'Por ahora solo se aceptan links de Instagram, Twitch y YouTube.',
    };
  }

  // Instagram: solo reels y posts de video. Perfiles y stories no.
  if (match.platform === 'instagram') {
    const okPath = /^\/(reel|reels|p|tv)\/[A-Za-z0-9_-]+\/?$/.test(u.pathname);
    if (!okPath) {
      return { ok: false, reason: 'De Instagram solo se aceptan Reels (instagram.com/reel/...).' };
    }
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
    return { ok: false, reason: 'Los envíos están cerrados (el stream está offline).' };
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
