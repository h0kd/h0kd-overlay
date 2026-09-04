import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  signPayload,
  verifyPayload,
  encryptToken,
  decryptToken,
  timingSafeEqual,
  b64urlEncode,
  pairingCode,
} from '../src/crypto.ts';

const SECRET = 'secreto-de-prueba';
const KEY = b64urlEncode(new Uint8Array(32).fill(7));

test('un payload firmado vuelve intacto', async () => {
  const token = await signPayload(SECRET, { uid: '123', login: 'h0kd' });
  assert.deepEqual(await verifyPayload(SECRET, token), { uid: '123', login: 'h0kd' });
});

test('una firma con otro secreto no valida', async () => {
  const token = await signPayload(SECRET, { uid: '123' });
  assert.equal(await verifyPayload('otro-secreto', token), null);
});

test('manipular el payload invalida la firma', async () => {
  const token = await signPayload(SECRET, { uid: '123', role: 'viewer' });
  const [body, sig] = token.split('.');
  const forged = b64urlEncode(new TextEncoder().encode(JSON.stringify({ uid: '999', role: 'broadcaster' })));
  assert.notEqual(body, forged);
  assert.equal(await verifyPayload(SECRET, `${forged}.${sig}`), null);
});

test('un token sin firma no pasa', async () => {
  assert.equal(await verifyPayload(SECRET, 'solo-payload'), null);
  assert.equal(await verifyPayload(SECRET, ''), null);
});

test('los tokens del broadcaster se cifran y descifran', async () => {
  const secretToken = 'oauth-access-token-de-twitch';
  const blob = await encryptToken(KEY, secretToken);
  assert.notEqual(blob, secretToken);
  assert.ok(!blob.includes(secretToken));
  assert.equal(await decryptToken(KEY, blob), secretToken);
});

test('descifrar con otra clave devuelve null en vez de tirar', async () => {
  const blob = await encryptToken(KEY, 'algo');
  const otherKey = b64urlEncode(new Uint8Array(32).fill(9));
  assert.equal(await decryptToken(otherKey, blob), null);
});

test('cifrar dos veces lo mismo da distinto (IV aleatorio)', async () => {
  const a = await encryptToken(KEY, 'mismo-valor');
  const b = await encryptToken(KEY, 'mismo-valor');
  assert.notEqual(a, b);
});

test('timingSafeEqual compara por contenido', () => {
  assert.equal(timingSafeEqual('abc', 'abc'), true);
  assert.equal(timingSafeEqual('abc', 'abd'), false);
  assert.equal(timingSafeEqual('abc', 'abcd'), false);
});

test('el código de emparejamiento evita caracteres ambiguos', () => {
  for (let i = 0; i < 200; i++) {
    const code = pairingCode();
    assert.equal(code.length, 8);
    assert.match(code, /^[23456789ABCDEFGHJKMNPQRSTUVWXYZ]{8}$/);
  }
});
