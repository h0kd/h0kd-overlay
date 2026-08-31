/**
 * Contrato de mensajes DO <-> agente.
 *
 * Espejo en TypeScript de `docs/ws-protocol.md`. Ese documento manda: si algo
 * difiere, se corrige acá, no allá. El agente implementa lo mismo con serde.
 */

export const PROTOCOL_VERSION = 1;
export const WS_SUBPROTOCOL = 'h0kd-vr.1';

/** Tope de tamaño por mensaje. Más grande que esto se descarta. */
export const MAX_MESSAGE_BYTES = 64 * 1024;

export type Platform = 'instagram' | 'twitch' | 'youtube';

export type ItemStatus =
  | 'submitted'
  | 'pending_review'
  | 'rejected_auto'
  | 'rejected'
  | 'approved'
  | 'downloading'
  | 'ready'
  | 'playing'
  | 'played'
  | 'failed'
  | 'cleared';

export type ErrorCode =
  // Extracción / plataforma
  | 'cookies_expired'
  | 'rate_limited'
  | 'extractor_failed'
  | 'unsupported_platform'
  | 'not_found'
  // Política
  | 'too_long'
  | 'too_large'
  | 'timeout'
  // Pipeline local
  | 'download_failed'
  | 'probe_failed'
  | 'transcode_failed'
  | 'disk_full'
  | 'binary_missing'
  // Protocolo
  | 'unsupported_type'
  | 'message_too_large'
  | 'cancelled';

export interface ErrorDetail {
  code: ErrorCode;
  /** En español: se muestra tal cual al mod y en la UI del agente. */
  message: string;
  retryable: boolean;
}

export interface ChannelSettings {
  submissions_open: boolean;
  max_duration_seconds: number;
  max_resolution: '720' | '1080';
  max_filesize_mb: number;
  playback_gap_seconds: number;
}

export interface CookieStatus {
  present: boolean;
  state: 'ok' | 'expired' | 'missing';
  last_ok_at: number | null;
  last_error_at: number | null;
}

export interface Envelope<T extends string, P> {
  v: typeof PROTOCOL_VERSION;
  type: T;
  id: string;
  ref?: string;
  ts: number;
  payload: P;
}

// ── DO -> agente ─────────────────────────────────────────────────────────────

export type Hello = Envelope<'hello', {
  channel_id: string;
  channel_login: string;
  protocol_version: typeof PROTOCOL_VERSION;
  server_time: number;
  settings: ChannelSettings;
  stream_online: boolean;
}>;

export type MetadataRequest = Envelope<'metadata.request', {
  item_id: string;
  source_url: string;
  platform: Platform;
}>;

export type DownloadRequest = Envelope<'download.request', {
  item_id: string;
  source_url: string;
  platform: Platform;
  max_resolution: '720' | '1080';
  approved_by: string;
}>;

export type Cancel = Envelope<'cancel', {
  item_id: string;
  reason: 'mod_rejected' | 'submitter_deleted' | 'admin' | 'expired';
}>;

export type Clear = Envelope<'clear', {
  reason: 'stream_offline' | 'admin';
  keep_now_playing: boolean;
}>;

export type SettingsUpdate = Envelope<'settings.update', { settings: ChannelSettings }>;

export interface ResyncItem {
  item_id: string;
  source_url: string;
  platform: Platform;
  status: 'pending_review' | 'approved' | 'downloading' | 'ready';
  position: number;
}

export type Resync = Envelope<'resync', { items: ResyncItem[] }>;

export type ToAgent =
  | Hello
  | MetadataRequest
  | DownloadRequest
  | Cancel
  | Clear
  | SettingsUpdate
  | Resync;

// ── agente -> DO ─────────────────────────────────────────────────────────────

export type AgentReady = Envelope<'agent.ready', {
  agent_version: string;
  ytdlp_version: string | null;
  ffmpeg_version: string | null;
  cookies: CookieStatus;
  encoder: 'h264_nvenc' | 'libx264';
  overlay_connected: boolean;
}>;

export type MetadataResult = Envelope<'metadata.result', {
  item_id: string;
  ok: boolean;
  title?: string;
  duration_seconds?: number;
  thumbnail_url?: string;
  uploader?: string;
  error?: ErrorDetail;
}>;

export type DownloadProgress = Envelope<'download.progress', {
  item_id: string;
  stage: 'downloading' | 'probing' | 'transcoding';
  percent: number | null;
}>;

export type DownloadResult = Envelope<'download.result', {
  item_id: string;
  ok: boolean;
  duration_seconds?: number;
  width?: number;
  height?: number;
  error?: ErrorDetail;
}>;

export type PlaybackStarted = Envelope<'playback.started', { item_id: string }>;

export type PlaybackEnded = Envelope<'playback.ended', {
  item_id: string;
  reason: 'ended' | 'error' | 'cancelled' | 'cleared';
  played_seconds: number;
}>;

export type AgentStatus = Envelope<'status', {
  queue_len: number;
  now_playing: string | null;
  cookies: CookieStatus;
  ytdlp_version: string | null;
  disk_free_mb: number;
  last_error: ErrorDetail | null;
}>;

export type AgentError = Envelope<'error', {
  item_id?: string;
  detail: ErrorDetail;
}>;

export type FromAgent =
  | AgentReady
  | MetadataResult
  | DownloadProgress
  | DownloadResult
  | PlaybackStarted
  | PlaybackEnded
  | AgentStatus
  | AgentError;

// ── Códigos de cierre ────────────────────────────────────────────────────────

export const CLOSE = {
  /** Token inválido o revocado. El agente NO debe reintentar. */
  UNAUTHORIZED: 4401,
  /** Otra conexión tomó el canal. El agente NO debe reintentar. */
  SUPERSEDED: 4409,
  /** Versión de protocolo no soportada. El agente NO debe reintentar. */
  BAD_VERSION: 4426,
  /** Demasiadas reconexiones: reintentar con el backoff al tope. */
  RATE_LIMITED: 4429,
} as const;

// ── Helpers ──────────────────────────────────────────────────────────────────

export function envelope<T extends string, P>(type: T, payload: P, ref?: string): Envelope<T, P> {
  return {
    v: PROTOCOL_VERSION,
    type,
    id: crypto.randomUUID(),
    ...(ref ? { ref } : {}),
    ts: Date.now(),
    payload,
  };
}

/**
 * Parseo defensivo de un mensaje entrante del agente. Devuelve null en vez de
 * tirar: un mensaje malformado no puede tumbar el hub de un canal en vivo.
 */
export function parseFromAgent(raw: string): FromAgent | null {
  if (raw.length > MAX_MESSAGE_BYTES) return null;
  let msg: unknown;
  try {
    msg = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof msg !== 'object' || msg === null) return null;
  const m = msg as Record<string, unknown>;
  if (m['v'] !== PROTOCOL_VERSION) return null;
  if (typeof m['type'] !== 'string' || typeof m['id'] !== 'string') return null;
  if (typeof m['payload'] !== 'object' || m['payload'] === null) return null;
  return msg as FromAgent;
}
