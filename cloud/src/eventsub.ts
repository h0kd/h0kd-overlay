/**
 * Webhook de EventSub: inicio y fin de stream.
 *
 * Twitch firma cada request; verificar esa firma es lo único que separa este
 * endpoint de "cualquiera en internet puede cerrarle los envíos a un canal".
 */

import { hmacHex, timingSafeEqual } from './crypto.ts';
import type { Env } from './env.ts';

const MSG_ID = 'twitch-eventsub-message-id';
const MSG_TS = 'twitch-eventsub-message-timestamp';
const MSG_SIG = 'twitch-eventsub-message-signature';
const MSG_TYPE = 'twitch-eventsub-message-type';

/** Ventana de replay que acepta Twitch para sus propios mensajes. */
const MAX_AGE_MS = 10 * 60 * 1000;

export type EventSubResult =
  | { kind: 'challenge'; challenge: string }
  | { kind: 'notification'; type: string; broadcasterId: string }
  | { kind: 'revocation'; type: string; broadcasterId: string }
  | { kind: 'ignored' }
  | { kind: 'invalid'; reason: string };

/**
 * Verifica y clasifica un request de EventSub.
 *
 * Recibe el body como texto crudo a propósito: la firma se calcula sobre los
 * bytes exactos que mandó Twitch, así que re-serializar el JSON la rompería.
 */
export async function handleEventSub(
  env: Env,
  headers: Headers,
  rawBody: string,
): Promise<EventSubResult> {
  const id = headers.get(MSG_ID);
  const ts = headers.get(MSG_TS);
  const sig = headers.get(MSG_SIG);
  const type = headers.get(MSG_TYPE);
  if (!id || !ts || !sig || !type) return { kind: 'invalid', reason: 'faltan headers' };

  const age = Date.now() - Date.parse(ts);
  if (!Number.isFinite(age) || Math.abs(age) > MAX_AGE_MS) {
    return { kind: 'invalid', reason: 'timestamp fuera de ventana' };
  }

  const expected = 'sha256=' + (await hmacHex(env.EVENTSUB_SECRET, id + ts + rawBody));
  if (!timingSafeEqual(sig, expected)) return { kind: 'invalid', reason: 'firma inválida' };

  let body: {
    challenge?: string;
    subscription?: { type?: string; condition?: { broadcaster_user_id?: string } };
  };
  try {
    body = JSON.parse(rawBody);
  } catch {
    return { kind: 'invalid', reason: 'body no es JSON' };
  }

  const subType = body.subscription?.type ?? '';
  const broadcasterId = body.subscription?.condition?.broadcaster_user_id ?? '';

  switch (type) {
    case 'webhook_callback_verification':
      return body.challenge
        ? { kind: 'challenge', challenge: body.challenge }
        : { kind: 'invalid', reason: 'verificación sin challenge' };
    case 'notification':
      if (!broadcasterId) return { kind: 'invalid', reason: 'notificación sin broadcaster' };
      return { kind: 'notification', type: subType, broadcasterId };
    case 'revocation':
      return { kind: 'revocation', type: subType, broadcasterId };
    default:
      return { kind: 'ignored' };
  }
}
