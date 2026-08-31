import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkUrl, checkPolicy } from '../src/policy.ts';

test('acepta un reel de Instagram y normaliza la URL', () => {
  const r = checkUrl('https://www.instagram.com/reel/AbC123_x-y/?igsh=tracking&utm_source=ig');
  assert.equal(r.ok, true);
  if (!r.ok) return;
  assert.equal(r.platform, 'instagram');
  // La query se descarta: ahí viven el tracking y los intentos de colar cosas.
  assert.equal(r.url, 'https://instagram.com/reel/AbC123_x-y/');
});

test('acepta twitch y youtube', () => {
  for (const [url, platform] of [
    ['https://clips.twitch.tv/SomeClipSlug', 'twitch'],
    ['https://www.youtube.com/watch', 'youtube'],
    ['https://youtu.be/abc', 'youtube'],
  ] as const) {
    const r = checkUrl(url);
    assert.equal(r.ok, true, url);
    if (r.ok) assert.equal(r.platform, platform);
  }
});

test('rechaza dominios fuera de la allowlist', () => {
  for (const url of [
    'https://evil.example.com/video.mp4',
    'https://instagram.com.evil.com/reel/x/',
    'https://notinstagram.com/reel/x/',
  ]) {
    assert.equal(checkUrl(url).ok, false, url);
  }
});

test('rechaza esquemas y formas peligrosas', () => {
  for (const url of [
    'http://instagram.com/reel/x/',            // sin https
    'file:///etc/passwd',
    'https://user:pass@instagram.com/reel/x/', // credenciales embebidas
    'https://instagram.com:8080/reel/x/',      // puerto explícito
    'not a url',
    '',
  ]) {
    assert.equal(checkUrl(url).ok, false, JSON.stringify(url));
  }
});

test('de Instagram solo pasan reels y posts, no perfiles', () => {
  assert.equal(checkUrl('https://instagram.com/algun_usuario').ok, false);
  assert.equal(checkUrl('https://instagram.com/stories/user/123/').ok, false);
  assert.equal(checkUrl('https://instagram.com/p/AbC123/').ok, true);
});

test('la política cierra envíos con el stream offline', () => {
  const r = checkPolicy(
    { submissions_open: false, cooldown_seconds: 60, max_pending_per_user: 3 },
    { last_submit_at: null, pending_count: 0 },
    Date.now(),
  );
  assert.equal(r.ok, false);
});

test('la política respeta el cooldown y el máximo en cola', () => {
  const now = Date.now();
  const policy = { submissions_open: true, cooldown_seconds: 60, max_pending_per_user: 3 };

  assert.equal(checkPolicy(policy, { last_submit_at: null, pending_count: 0 }, now).ok, true);
  assert.equal(checkPolicy(policy, { last_submit_at: now - 30_000, pending_count: 0 }, now).ok, false);
  assert.equal(checkPolicy(policy, { last_submit_at: now - 61_000, pending_count: 0 }, now).ok, true);
  assert.equal(checkPolicy(policy, { last_submit_at: null, pending_count: 3 }, now).ok, false);
});
