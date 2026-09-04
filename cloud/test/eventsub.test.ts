import { test } from 'node:test';
import assert from 'node:assert/strict';
import { handleEventSub } from '../src/eventsub.ts';
import { hmacHex } from '../src/crypto.ts';
import type { Env } from '../src/env.ts';

const SECRET = 'eventsub-secreto';
const env = { EVENTSUB_SECRET: SECRET } as Env;

async function signed(type: string, body: string, opts: { ts?: string; sig?: string } = {}) {
  const id = 'msg-1';
  const ts = opts.ts ?? new Date().toISOString();
  const sig = opts.sig ?? 'sha256=' + (await hmacHex(SECRET, id + ts + body));
  return new Headers({
    'twitch-eventsub-message-id': id,
    'twitch-eventsub-message-timestamp': ts,
    'twitch-eventsub-message-signature': sig,
    'twitch-eventsub-message-type': type,
  });
}

const notification = JSON.stringify({
  subscription: { type: 'stream.online', condition: { broadcaster_user_id: '12345' } },
});

test('acepta una notificación bien firmada', async () => {
  const r = await handleEventSub(env, await signed('notification', notification), notification);
  assert.deepEqual(r, { kind: 'notification', type: 'stream.online', broadcasterId: '12345' });
});

test('responde el challenge de verificación', async () => {
  const body = JSON.stringify({ challenge: 'abc123', subscription: { type: 'stream.online' } });
  const r = await handleEventSub(env, await signed('webhook_callback_verification', body), body);
  assert.deepEqual(r, { kind: 'challenge', challenge: 'abc123' });
});

test('rechaza una firma inválida', async () => {
  const headers = await signed('notification', notification, { sig: 'sha256=' + '0'.repeat(64) });
  const r = await handleEventSub(env, headers, notification);
  assert.equal(r.kind, 'invalid');
});

test('rechaza un body alterado despues de firmar', async () => {
  const headers = await signed('notification', notification);
  const tampered = JSON.stringify({
    subscription: { type: 'stream.offline', condition: { broadcaster_user_id: '999' } },
  });
  const r = await handleEventSub(env, headers, tampered);
  assert.equal(r.kind, 'invalid');
});

test('rechaza un replay viejo aunque la firma cierre', async () => {
  const old = new Date(Date.now() - 30 * 60 * 1000).toISOString();
  const headers = await signed('notification', notification, { ts: old });
  const r = await handleEventSub(env, headers, notification);
  assert.equal(r.kind, 'invalid');
  if (r.kind === 'invalid') assert.match(r.reason, /timestamp/);
});

test('rechaza requests sin headers de Twitch', async () => {
  const r = await handleEventSub(env, new Headers(), notification);
  assert.equal(r.kind, 'invalid');
});

test('reconoce una revocación', async () => {
  const r = await handleEventSub(env, await signed('revocation', notification), notification);
  assert.equal(r.kind, 'revocation');
});
