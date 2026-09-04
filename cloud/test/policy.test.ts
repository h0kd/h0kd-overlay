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
    ['https://www.youtube.com/watch?v=aB3xKq9_-1', 'youtube'],
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

test('de Instagram solo pasan reels: ni perfiles, ni stories, ni posts', () => {
  assert.equal(checkUrl('https://instagram.com/algun_usuario').ok, false);
  assert.equal(checkUrl('https://instagram.com/stories/user/123/').ok, false);
  assert.equal(checkUrl('https://instagram.com/reels/AbC123/').ok, true);
  // Un /p/ puede ser video, pero casi siempre es foto y el viewer se entera
  // recién cuando falla. Se rechaza al enviar, diciendo cuál es el link bueno.
  for (const u of ['https://instagram.com/p/AbC123/', 'https://instagram.com/tv/AbC123/']) {
    const r = checkUrl(u);
    assert.equal(r.ok, false, u);
    if (!r.ok) assert.match(r.reason, /Reels/);
  }
});

test('de YouTube conserva el id del video y tira el resto', () => {
  const r = checkUrl('https://www.youtube.com/watch?v=aB3xKq9_-1&list=PL123&t=42s');
  assert.equal(r.ok, true);
  if (r.ok) {
    assert.equal(r.platform, 'youtube');
    assert.equal(r.url, 'https://youtube.com/watch?v=aB3xKq9_-1');
  }
  assert.equal(checkUrl('https://youtube.com/watch').ok, false);
  assert.equal(checkUrl('https://youtube.com/watch?list=PL123').ok, false);
  assert.equal(checkUrl('https://youtu.be/aB3xKq9_-1').ok, true);
});

test('acepta las dos formas de link de TikTok', () => {
  for (const u of [
    'https://www.tiktok.com/@alguien/video/7412345678901234567',
    'https://tiktok.com/t/ZM8abc123',
    'https://vm.tiktok.com/ZM8abc123',
    'https://vt.tiktok.com/ZM8abc123/',
  ]) {
    const r = checkUrl(u);
    assert.equal(r.ok, true, u);
    if (r.ok) assert.equal(r.platform, 'tiktok');
  }
});

test('el link largo de TikTok conserva el www que yt-dlp exige', () => {
  const r = checkUrl('https://tiktok.com/@alguien/video/7412345678901234567?is_from_webapp=1');
  assert.equal(r.ok, true);
  if (r.ok) assert.equal(r.url, 'https://www.tiktok.com/@alguien/video/7412345678901234567');

  // El corto redirige solo y su host es el que vale.
  const c = checkUrl('https://vm.tiktok.com/ZM8abc123');
  assert.equal(c.ok, true);
  if (c.ok) assert.equal(c.url, 'https://vm.tiktok.com/ZM8abc123');
});

test('de X acepta posts en cualquiera de sus formas y normaliza a x.com', () => {
  for (const u of [
    'https://x.com/NASA/status/1834334523344568597',
    'https://twitter.com/NASA/status/1834334523344568597',
    'https://mobile.twitter.com/NASA/status/1834334523344568597?s=20&t=abc',
    'https://x.com/NASA/status/1834334523344568597/video/1',
  ]) {
    const r = checkUrl(u);
    assert.equal(r.ok, true, u);
    if (r.ok) {
      assert.equal(r.platform, 'x');
      assert.equal(r.url, 'https://x.com/NASA/status/1834334523344568597');
    }
  }
  const i = checkUrl('https://x.com/i/status/1834334523344568597');
  assert.equal(i.ok, true);
  if (i.ok) assert.equal(i.url, 'https://x.com/i/status/1834334523344568597');
});

test('de X no pasan perfiles, búsquedas ni listas', () => {
  for (const u of [
    'https://x.com/NASA',
    'https://x.com/search?q=nasa',
    'https://x.com/i/lists/123',
    'https://x.com/NASA/status/',
    'https://x.com/NASA/status/abc',
    'https://x.com/home',
  ]) {
    assert.equal(checkUrl(u).ok, false, u);
  }
});

test('de TikTok no pasa un perfil', () => {
  for (const u of [
    'https://www.tiktok.com/@alguien',
    'https://www.tiktok.com/@alguien/video/',
    'https://www.tiktok.com/foo/bar/baz',
  ]) {
    assert.equal(checkUrl(u).ok, false, u);
  }
});

test('acepta un archivo de kappa.lol y lo normaliza al id', () => {
  for (const u of [
    'https://kappa.lol/qMiVeE',
    'https://kappa.lol/qMiVeE.mp4',
    'https://kappa.lol/qMiVeE/lo-que-sea.mp4?x=1',
    'https://www.kappa.lol/qMiVeE',
  ]) {
    const r = checkUrl(u);
    assert.equal(r.ok, true, u);
    if (r.ok) {
      assert.equal(r.platform, 'kappa');
      assert.equal(r.url, 'https://kappa.lol/qMiVeE');
    }
  }
});

test('de kappa.lol no pasan el index, las páginas del sitio ni los subdominios', () => {
  for (const u of [
    'https://kappa.lol/',
    'https://kappa.lol/uploaders',
    'https://kappa.lol/api/upload',
    'https://kappa.lol/delete?abc',
    'https://w.kappa.lol/qMiVeE',
    'https://kappa.lol/qM',
  ]) {
    assert.equal(checkUrl(u).ok, false, u);
  }
});

test('la política cierra envíos con el stream offline', () => {
  const r = checkPolicy(
    { submissions_open: false, stream_online: false, cooldown_seconds: 60, max_pending_per_user: 3 },
    { last_submit_at: null, pending_count: 0 },
    Date.now(),
  );
  assert.equal(r.ok, false);
  if (!r.ok) assert.match(r.reason, /offline/);
});

test('cerrados a mano en pleno stream no dice que el stream está offline', () => {
  const r = checkPolicy(
    { submissions_open: false, stream_online: true, cooldown_seconds: 60, max_pending_per_user: 3 },
    { last_submit_at: null, pending_count: 0 },
    Date.now(),
  );
  assert.equal(r.ok, false);
  if (!r.ok) {
    assert.doesNotMatch(r.reason, /offline/);
    assert.match(r.reason, /pausados/);
  }
});

test('la política respeta el cooldown y el máximo en cola', () => {
  const now = Date.now();
  const policy = { submissions_open: true, stream_online: true, cooldown_seconds: 60, max_pending_per_user: 3 };

  assert.equal(checkPolicy(policy, { last_submit_at: null, pending_count: 0 }, now).ok, true);
  assert.equal(checkPolicy(policy, { last_submit_at: now - 30_000, pending_count: 0 }, now).ok, false);
  assert.equal(checkPolicy(policy, { last_submit_at: now - 61_000, pending_count: 0 }, now).ok, true);
  assert.equal(checkPolicy(policy, { last_submit_at: null, pending_count: 3 }, now).ok, false);
});
