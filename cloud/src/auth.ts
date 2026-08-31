/**
 * Sesiones e identidad.
 *
 * Invariante del sistema: la identidad y el `channel_id` salen SIEMPRE de la
 * sesión firmada del lado del servidor. Ningún endpoint acepta un user id, un
 * login o un rol que venga del cliente. El formulario de envío solo lleva el
 * link; el "quién" lo pone el servidor.
 */

import type { Context } from 'hono';
import { getCookie, setCookie, deleteCookie } from 'hono/cookie';
import { signPayload, verifyPayload } from './crypto.ts';
import {
  OAUTH_COOKIE,
  OAUTH_TTL_SECONDS,
  SESSION_COOKIE,
  SESSION_TTL_SECONDS,
  type Env,
} from './env.ts';

export interface Session {
  /** twitch user id */
  uid: string;
  login: string;
  name: string;
  /** epoch segundos */
  exp: number;
}

export interface OAuthState {
  state: string;
  /** A dónde volver después del login. Solo path, nunca URL absoluta. */
  to: string;
  exp: number;
}

export type Role = 'broadcaster' | 'mod' | 'viewer';

const nowSec = () => Math.floor(Date.now() / 1000);

// ── Cookie de sesión ─────────────────────────────────────────────────────────

export async function setSession(
  c: Context<{ Bindings: Env }>,
  user: { id: string; login: string; display_name: string },
): Promise<void> {
  const session: Session = {
    uid: user.id,
    login: user.login,
    name: user.display_name || user.login,
    exp: nowSec() + SESSION_TTL_SECONDS,
  };
  const token = await signPayload(c.env.SESSION_SECRET, session);
  setCookie(c, SESSION_COOKIE, token, {
    httpOnly: true,
    secure: true,
    sameSite: 'Lax',
    path: '/',
    maxAge: SESSION_TTL_SECONDS,
  });
}

export async function getSession(c: Context<{ Bindings: Env }>): Promise<Session | null> {
  const raw = getCookie(c, SESSION_COOKIE);
  if (!raw) return null;
  const s = await verifyPayload<Session>(c.env.SESSION_SECRET, raw);
  if (!s || typeof s.uid !== 'string' || s.exp < nowSec()) return null;
  return s;
}

export function clearSession(c: Context<{ Bindings: Env }>): void {
  deleteCookie(c, SESSION_COOKIE, { path: '/' });
}

// ── `state` de OAuth (anti-CSRF) ─────────────────────────────────────────────

/**
 * El `state` se guarda firmado en una cookie propia además de viajar en la URL.
 * Al volver, los dos tienen que coincidir: eso ata el callback al navegador que
 * inició el login y corta el CSRF de inicio de sesión.
 */
export async function beginOAuth(c: Context<{ Bindings: Env }>, to: string): Promise<string> {
  const state = crypto.randomUUID();
  const payload: OAuthState = { state, to: safeRedirect(to), exp: nowSec() + OAUTH_TTL_SECONDS };
  const token = await signPayload(c.env.SESSION_SECRET, payload);
  setCookie(c, OAUTH_COOKIE, token, {
    httpOnly: true,
    secure: true,
    sameSite: 'Lax',
    path: '/',
    maxAge: OAUTH_TTL_SECONDS,
  });
  return state;
}

export async function consumeOAuth(
  c: Context<{ Bindings: Env }>,
  state: string,
): Promise<OAuthState | null> {
  const raw = getCookie(c, OAUTH_COOKIE);
  deleteCookie(c, OAUTH_COOKIE, { path: '/' });
  if (!raw) return null;
  const s = await verifyPayload<OAuthState>(c.env.SESSION_SECRET, raw);
  if (!s || s.exp < nowSec() || s.state !== state) return null;
  return s;
}

/**
 * Solo se permite volver a un path de este mismo sitio. Un `to` absoluto
 * convertiría el login en un open redirect utilizable para phishing.
 */
export function safeRedirect(to: string): string {
  if (!to.startsWith('/') || to.startsWith('//')) return '/';
  return to;
}

// ── Roles ────────────────────────────────────────────────────────────────────

/**
 * Resuelve el rol del usuario EN ESE canal. El rol de broadcaster se decide
 * comparando el user id con el channel_id; el de mod, consultando la tabla que
 * el broadcaster administra desde /admin (no la lista de Twitch: ser mod del
 * canal no da acceso automático a esto).
 */
export async function roleFor(
  env: Env,
  session: Session | null,
  channelId: string,
): Promise<Role | null> {
  if (!session) return null;
  if (session.uid === channelId) return 'broadcaster';
  const row = await env.DB.prepare(
    'SELECT 1 FROM authorized_mods WHERE channel_id = ? AND twitch_user_id = ?',
  )
    .bind(channelId, session.uid)
    .first();
  return row ? 'mod' : 'viewer';
}
