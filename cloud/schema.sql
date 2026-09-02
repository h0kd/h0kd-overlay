-- Schema de D1 para Video Requests.
-- Aplicar con:  npm run db:local   /   npm run db:remote
--
-- Regla que atraviesa todo el schema: `channel_id` (el user id de Twitch del
-- broadcaster) va en TODAS las tablas y toda query filtra por él. El valor sale
-- siempre de la sesión firmada o del token del agente, nunca de input del
-- cliente.

CREATE TABLE IF NOT EXISTS channels (
  channel_id      TEXT PRIMARY KEY,   -- twitch broadcaster user id
  twitch_login    TEXT NOT NULL,
  agent_paired_at INTEGER,
  created_at      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS authorized_mods (
  channel_id     TEXT NOT NULL,
  twitch_user_id TEXT NOT NULL,
  twitch_login   TEXT NOT NULL,
  added_at       INTEGER NOT NULL,
  PRIMARY KEY (channel_id, twitch_user_id)
);

CREATE TABLE IF NOT EXISTS channel_settings (
  channel_id           TEXT PRIMARY KEY,
  submissions_open     INTEGER NOT NULL DEFAULT 0,
  -- Lo último que dijo EventSub del stream. Va aparte de submissions_open
  -- porque el interruptor manual de /admin toca solo ese. (Bases anteriores
  -- al 2026-09-01: aplicar migrations/0001-stream-online.sql.)
  stream_online        INTEGER NOT NULL DEFAULT 0,
  who_can_submit       TEXT    NOT NULL DEFAULT 'everyone',
  cooldown_seconds     INTEGER NOT NULL DEFAULT 60,
  max_pending_per_user INTEGER NOT NULL DEFAULT 3,
  max_resolution       TEXT    NOT NULL DEFAULT '720',
  max_duration_seconds INTEGER NOT NULL DEFAULT 30,
  max_filesize_mb      INTEGER NOT NULL DEFAULT 100,
  playback_gap_seconds INTEGER NOT NULL DEFAULT 5
);

CREATE TABLE IF NOT EXISTS queue_items (
  id                  TEXT PRIMARY KEY,  -- uuid
  channel_id          TEXT NOT NULL,
  submitter_twitch_id TEXT NOT NULL,
  submitter_login     TEXT NOT NULL,
  source_url          TEXT NOT NULL,
  platform            TEXT NOT NULL,     -- instagram | twitch | youtube
  status              TEXT NOT NULL,
  title               TEXT,
  thumbnail_url       TEXT,
  duration_seconds    INTEGER,
  decided_by          TEXT,
  decided_at          INTEGER,
  decided_reason      TEXT,              -- motivo del rechazo, lo escribe el mod
  created_at          INTEGER NOT NULL,
  error               TEXT
);
CREATE INDEX IF NOT EXISTS idx_queue_channel_status ON queue_items(channel_id, status);
CREATE INDEX IF NOT EXISTS idx_queue_submitter      ON queue_items(channel_id, submitter_twitch_id, created_at);

-- Fotos de perfil de Twitch, guardadas por login.
--
-- No es una tabla de usuarios: es solo para no tener que preguntarle una foto a
-- Helix cada vez que se pinta una lista. Se refresca sola cada vez que alguien
-- abre una página, manda un link o decide algo, así que el que participa tiene
-- su foto al día y el que no, no le importa a nadie.
CREATE TABLE IF NOT EXISTS user_pics (
  login      TEXT PRIMARY KEY,   -- siempre en minúsculas
  pic        TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- User token del broadcaster, para llamadas a Helix (Get Moderators).
-- access_token y refresh_token se guardan CIFRADOS con TOKEN_ENC_KEY: si
-- alguien lee la base, no se lleva sesiones de Twitch utilizables.
CREATE TABLE IF NOT EXISTS broadcaster_tokens (
  channel_id    TEXT PRIMARY KEY,
  access_token  TEXT NOT NULL,
  refresh_token TEXT NOT NULL,
  expires_at    INTEGER NOT NULL
);

-- Emparejamiento del agente. El código es de un solo uso y dura minutos; el
-- token que entrega se guarda HASHEADO (sha-256), así la base nunca contiene
-- un token utilizable tal cual.
CREATE TABLE IF NOT EXISTS pairing_codes (
  code       TEXT PRIMARY KEY,
  channel_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  used_at    INTEGER
);

CREATE TABLE IF NOT EXISTS agent_tokens (
  token_hash   TEXT PRIMARY KEY,
  channel_id   TEXT NOT NULL,
  created_at   INTEGER NOT NULL,
  last_seen_at INTEGER,
  revoked_at   INTEGER
);
CREATE INDEX IF NOT EXISTS idx_agent_tokens_channel ON agent_tokens(channel_id);
