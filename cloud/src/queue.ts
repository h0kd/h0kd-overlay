/**
 * Acceso a D1: cola, canales, settings y mods.
 *
 * D1 es la fuente de verdad de los estados. El agente reporta hechos ("esto
 * dura 14 segundos", "esto empezó a sonar") y acá se derivan los estados; así,
 * si el agente se cae, el estado verdadero sigue completo.
 *
 * Toda query lleva `channel_id` en el WHERE. No hay atajos: el aislamiento
 * entre canales depende de eso.
 */

import type { ChannelSettings, ItemStatus, Platform } from './protocol.ts';

export interface QueueItem {
  id: string;
  channel_id: string;
  submitter_twitch_id: string;
  submitter_login: string;
  source_url: string;
  platform: Platform;
  status: ItemStatus;
  title: string | null;
  thumbnail_url: string | null;
  duration_seconds: number | null;
  decided_by: string | null;
  decided_at: number | null;
  created_at: number;
  error: string | null;
}

/** Estados en los que un ítem todavía "cuenta" para los límites del usuario. */
export const LIVE_STATUSES: ItemStatus[] = [
  'submitted',
  'pending_review',
  'approved',
  'downloading',
  'ready',
  'playing',
];

const DEFAULT_SETTINGS: ChannelSettings = {
  submissions_open: false,
  max_duration_seconds: 30,
  max_resolution: '720',
  max_filesize_mb: 100,
  playback_gap_seconds: 5,
};

// ── Canales ──────────────────────────────────────────────────────────────────

export async function ensureChannel(db: D1Database, channelId: string, login: string) {
  const now = Date.now();
  await db
    .prepare(
      `INSERT INTO channels (channel_id, twitch_login, created_at) VALUES (?, ?, ?)
       ON CONFLICT(channel_id) DO UPDATE SET twitch_login = excluded.twitch_login`,
    )
    .bind(channelId, login, now)
    .run();
  await db
    .prepare(`INSERT OR IGNORE INTO channel_settings (channel_id) VALUES (?)`)
    .bind(channelId)
    .run();
}

export async function channelByLogin(db: D1Database, login: string) {
  return db
    .prepare('SELECT channel_id, twitch_login FROM channels WHERE twitch_login = ?')
    .bind(login.toLowerCase())
    .first<{ channel_id: string; twitch_login: string }>();
}

export async function channelById(db: D1Database, channelId: string) {
  return db
    .prepare('SELECT channel_id, twitch_login FROM channels WHERE channel_id = ?')
    .bind(channelId)
    .first<{ channel_id: string; twitch_login: string }>();
}

// ── Settings ─────────────────────────────────────────────────────────────────

interface SettingsRow {
  submissions_open: number;
  who_can_submit: string;
  cooldown_seconds: number;
  max_pending_per_user: number;
  max_resolution: string;
  max_duration_seconds: number;
  max_filesize_mb: number;
  playback_gap_seconds: number;
}

export interface FullSettings extends ChannelSettings {
  who_can_submit: string;
  cooldown_seconds: number;
  max_pending_per_user: number;
}

export async function getSettings(db: D1Database, channelId: string): Promise<FullSettings> {
  const row = await db
    .prepare('SELECT * FROM channel_settings WHERE channel_id = ?')
    .bind(channelId)
    .first<SettingsRow>();
  if (!row) {
    return { ...DEFAULT_SETTINGS, who_can_submit: 'everyone', cooldown_seconds: 60, max_pending_per_user: 3 };
  }
  return {
    submissions_open: row.submissions_open === 1,
    max_duration_seconds: row.max_duration_seconds,
    max_resolution: row.max_resolution === '1080' ? '1080' : '720',
    max_filesize_mb: row.max_filesize_mb,
    playback_gap_seconds: row.playback_gap_seconds,
    who_can_submit: row.who_can_submit,
    cooldown_seconds: row.cooldown_seconds,
    max_pending_per_user: row.max_pending_per_user,
  };
}

export async function setSubmissionsOpen(db: D1Database, channelId: string, open: boolean) {
  await db
    .prepare('UPDATE channel_settings SET submissions_open = ? WHERE channel_id = ?')
    .bind(open ? 1 : 0, channelId)
    .run();
}

const NUMERIC_SETTINGS = [
  'cooldown_seconds',
  'max_pending_per_user',
  'max_duration_seconds',
  'max_filesize_mb',
  'playback_gap_seconds',
] as const;

const LIMITS: Record<(typeof NUMERIC_SETTINGS)[number], [number, number]> = {
  cooldown_seconds: [0, 3600],
  max_pending_per_user: [1, 20],
  max_duration_seconds: [5, 120],
  max_filesize_mb: [1, 500],
  playback_gap_seconds: [0, 60],
};

/**
 * Actualiza settings desde /admin. Solo se aceptan claves conocidas y valores
 * dentro de rango: es input de un humano por HTTP, aunque ese humano sea el
 * dueño del canal.
 */
export async function updateSettings(
  db: D1Database,
  channelId: string,
  patch: Record<string, unknown>,
): Promise<void> {
  const sets: string[] = [];
  const vals: unknown[] = [];

  for (const key of NUMERIC_SETTINGS) {
    const v = patch[key];
    if (v === undefined) continue;
    const n = Math.floor(Number(v));
    if (!Number.isFinite(n)) continue;
    const [lo, hi] = LIMITS[key];
    sets.push(`${key} = ?`);
    vals.push(Math.min(hi, Math.max(lo, n)));
  }
  if (patch['max_resolution'] !== undefined) {
    sets.push('max_resolution = ?');
    vals.push(patch['max_resolution'] === '1080' ? '1080' : '720');
  }
  if (patch['who_can_submit'] !== undefined) {
    const allowed = ['everyone', 'followers', 'subscribers', 'vips'];
    const v = String(patch['who_can_submit']);
    sets.push('who_can_submit = ?');
    vals.push(allowed.includes(v) ? v : 'everyone');
  }
  if (!sets.length) return;

  vals.push(channelId);
  await db
    .prepare(`UPDATE channel_settings SET ${sets.join(', ')} WHERE channel_id = ?`)
    .bind(...vals)
    .run();
}

// ── Cola ─────────────────────────────────────────────────────────────────────

export async function submitterState(db: D1Database, channelId: string, userId: string) {
  const placeholders = LIVE_STATUSES.map(() => '?').join(',');
  const pending = await db
    .prepare(
      `SELECT COUNT(*) AS n FROM queue_items
       WHERE channel_id = ? AND submitter_twitch_id = ? AND status IN (${placeholders})`,
    )
    .bind(channelId, userId, ...LIVE_STATUSES)
    .first<{ n: number }>();
  const last = await db
    .prepare(
      `SELECT MAX(created_at) AS t FROM queue_items
       WHERE channel_id = ? AND submitter_twitch_id = ?`,
    )
    .bind(channelId, userId)
    .first<{ t: number | null }>();
  return {
    pending_count: pending?.n ?? 0,
    last_submit_at: last?.t ?? null,
  };
}

export async function insertItem(db: D1Database, item: {
  id: string;
  channel_id: string;
  submitter_twitch_id: string;
  submitter_login: string;
  source_url: string;
  platform: Platform;
}): Promise<void> {
  await db
    .prepare(
      `INSERT INTO queue_items
         (id, channel_id, submitter_twitch_id, submitter_login, source_url, platform, status, created_at)
       VALUES (?, ?, ?, ?, ?, ?, 'submitted', ?)`,
    )
    .bind(
      item.id,
      item.channel_id,
      item.submitter_twitch_id,
      item.submitter_login,
      item.source_url,
      item.platform,
      Date.now(),
    )
    .run();
}

export async function getItem(db: D1Database, channelId: string, id: string) {
  return db
    .prepare('SELECT * FROM queue_items WHERE channel_id = ? AND id = ?')
    .bind(channelId, id)
    .first<QueueItem>();
}

export async function listByStatus(
  db: D1Database,
  channelId: string,
  statuses: ItemStatus[],
): Promise<QueueItem[]> {
  if (!statuses.length) return [];
  const placeholders = statuses.map(() => '?').join(',');
  const res = await db
    .prepare(
      `SELECT * FROM queue_items
       WHERE channel_id = ? AND status IN (${placeholders})
       ORDER BY created_at ASC`,
    )
    .bind(channelId, ...statuses)
    .all<QueueItem>();
  return res.results ?? [];
}

export async function setStatus(
  db: D1Database,
  channelId: string,
  id: string,
  status: ItemStatus,
  extra: { error?: string | null; decided_by?: string } = {},
): Promise<void> {
  const sets = ['status = ?'];
  const vals: unknown[] = [status];
  if (extra.error !== undefined) {
    sets.push('error = ?');
    vals.push(extra.error);
  }
  if (extra.decided_by !== undefined) {
    sets.push('decided_by = ?', 'decided_at = ?');
    vals.push(extra.decided_by, Date.now());
  }
  vals.push(channelId, id);
  await db
    .prepare(`UPDATE queue_items SET ${sets.join(', ')} WHERE channel_id = ? AND id = ?`)
    .bind(...vals)
    .run();
}

export async function setMetadata(
  db: D1Database,
  channelId: string,
  id: string,
  meta: { title?: string; duration_seconds?: number; thumbnail_url?: string },
): Promise<void> {
  await db
    .prepare(
      `UPDATE queue_items SET title = ?, duration_seconds = ?, thumbnail_url = ?
       WHERE channel_id = ? AND id = ?`,
    )
    .bind(
      meta.title ?? null,
      meta.duration_seconds ?? null,
      meta.thumbnail_url ?? null,
      channelId,
      id,
    )
    .run();
}

/**
 * Fin de stream: todo lo que estaba en juego pasa a `cleared`. Lo ya
 * reproducido y lo rechazado se dejan como están, que son el historial.
 */
export async function clearQueue(db: D1Database, channelId: string): Promise<number> {
  const placeholders = LIVE_STATUSES.map(() => '?').join(',');
  const res = await db
    .prepare(
      `UPDATE queue_items SET status = 'cleared'
       WHERE channel_id = ? AND status IN (${placeholders})`,
    )
    .bind(channelId, ...LIVE_STATUSES)
    .run();
  return res.meta.changes ?? 0;
}

// ── Mods autorizados ─────────────────────────────────────────────────────────

export async function listAuthorizedMods(db: D1Database, channelId: string) {
  const res = await db
    .prepare('SELECT twitch_user_id, twitch_login FROM authorized_mods WHERE channel_id = ?')
    .bind(channelId)
    .all<{ twitch_user_id: string; twitch_login: string }>();
  return res.results ?? [];
}

export async function setModAuthorized(
  db: D1Database,
  channelId: string,
  userId: string,
  login: string,
  authorized: boolean,
): Promise<void> {
  if (authorized) {
    await db
      .prepare(
        `INSERT INTO authorized_mods (channel_id, twitch_user_id, twitch_login, added_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(channel_id, twitch_user_id) DO UPDATE SET twitch_login = excluded.twitch_login`,
      )
      .bind(channelId, userId, login, Date.now())
      .run();
  } else {
    await db
      .prepare('DELETE FROM authorized_mods WHERE channel_id = ? AND twitch_user_id = ?')
      .bind(channelId, userId)
      .run();
  }
}
