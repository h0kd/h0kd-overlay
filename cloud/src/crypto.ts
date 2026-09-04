/**
 * Primitivas criptográficas sobre WebCrypto. Sin dependencias.
 *
 * Tres usos:
 *   - firmar sesiones y el `state` de OAuth (HMAC-SHA256),
 *   - cifrar los tokens del broadcaster antes de escribirlos en D1 (AES-GCM),
 *   - verificar la firma de los webhooks de EventSub (HMAC-SHA256).
 */

const enc = new TextEncoder();
const dec = new TextDecoder();

// ── base64url ────────────────────────────────────────────────────────────────

export function b64urlEncode(bytes: Uint8Array): string {
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function b64urlDecode(s: string): Uint8Array {
  const pad = s.length % 4 === 0 ? '' : '='.repeat(4 - (s.length % 4));
  const bin = atob(s.replace(/-/g, '+').replace(/_/g, '/') + pad);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ── Comparación en tiempo constante ──────────────────────────────────────────

/**
 * Comparar firmas con `===` filtra información por el tiempo de respuesta:
 * corta en el primer byte distinto, así que un atacante puede ir adivinando
 * byte a byte. Esto siempre recorre todo.
 */
export function timingSafeEqual(a: string, b: string): boolean {
  const ab = enc.encode(a);
  const bb = enc.encode(b);
  // La longitud sí se filtra, y no importa: las firmas son de largo fijo.
  if (ab.length !== bb.length) return false;
  let diff = 0;
  for (let i = 0; i < ab.length; i++) diff |= ab[i]! ^ bb[i]!;
  return diff === 0;
}

// ── HMAC-SHA256 ──────────────────────────────────────────────────────────────

async function hmacKey(secret: string): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    'raw',
    enc.encode(secret),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign'],
  );
}

export async function hmac(secret: string, data: string): Promise<string> {
  const key = await hmacKey(secret);
  const sig = await crypto.subtle.sign('HMAC', key, enc.encode(data));
  return b64urlEncode(new Uint8Array(sig));
}

export async function hmacHex(secret: string, data: string): Promise<string> {
  const key = await hmacKey(secret);
  const sig = await crypto.subtle.sign('HMAC', key, enc.encode(data));
  return [...new Uint8Array(sig)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

export async function sha256Hex(data: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', enc.encode(data));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

// ── Payloads firmados (sesiones, state de OAuth) ─────────────────────────────

/**
 * Firma un objeto como `<payload>.<hmac>`. No es un JWT: no hay algoritmo
 * negociable en el mensaje, así que el ataque de `alg: none` no existe acá.
 */
export async function signPayload(secret: string, payload: unknown): Promise<string> {
  const body = b64urlEncode(enc.encode(JSON.stringify(payload)));
  const sig = await hmac(secret, body);
  return `${body}.${sig}`;
}

/** Verifica y devuelve el payload, o null si la firma no cierra. */
export async function verifyPayload<T>(secret: string, token: string): Promise<T | null> {
  const dot = token.lastIndexOf('.');
  if (dot <= 0) return null;
  const body = token.slice(0, dot);
  const sig = token.slice(dot + 1);
  const expected = await hmac(secret, body);
  if (!timingSafeEqual(sig, expected)) return null;
  try {
    return JSON.parse(dec.decode(b64urlDecode(body))) as T;
  } catch {
    return null;
  }
}

// ── AES-GCM para los tokens del broadcaster ──────────────────────────────────

async function aesKey(keyB64: string): Promise<CryptoKey> {
  const raw = b64urlDecode(keyB64.replace(/\+/g, '-').replace(/\//g, '_'));
  if (raw.length !== 32) {
    throw new Error('TOKEN_ENC_KEY debe ser de 32 bytes en base64');
  }
  return crypto.subtle.importKey('raw', raw, { name: 'AES-GCM' }, false, ['encrypt', 'decrypt']);
}

export async function encryptToken(keyB64: string, plaintext: string): Promise<string> {
  const key = await aesKey(keyB64);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, enc.encode(plaintext));
  const out = new Uint8Array(iv.length + ct.byteLength);
  out.set(iv, 0);
  out.set(new Uint8Array(ct), iv.length);
  return b64urlEncode(out);
}

export async function decryptToken(keyB64: string, blob: string): Promise<string | null> {
  try {
    const key = await aesKey(keyB64);
    const bytes = b64urlDecode(blob);
    const iv = bytes.slice(0, 12);
    const ct = bytes.slice(12);
    const pt = await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, key, ct);
    return dec.decode(pt);
  } catch {
    return null;
  }
}

// ── Generadores ──────────────────────────────────────────────────────────────

/** Token opaco de 32 bytes, para el agente. */
export function randomToken(): string {
  return b64urlEncode(crypto.getRandomValues(new Uint8Array(32)));
}

/**
 * Código de emparejamiento de 8 caracteres. Alfabeto sin 0/O/1/I/L, porque
 * alguien lo va a leer de una pantalla y tipearlo en otra.
 */
export function pairingCode(): string {
  const alphabet = '23456789ABCDEFGHJKMNPQRSTUVWXYZ';
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  let out = '';
  for (const b of bytes) out += alphabet[b % alphabet.length];
  return out;
}
