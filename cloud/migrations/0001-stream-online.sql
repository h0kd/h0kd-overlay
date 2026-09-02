-- 2026-09-01: estado del stream aparte del interruptor de envíos.
-- Aplicar UNA vez sobre bases creadas antes de esa fecha:
--   npx wrangler d1 execute video-requests --remote --file=./migrations/0001-stream-online.sql
-- schema.sql ya trae la columna para bases nuevas.
ALTER TABLE channel_settings ADD COLUMN stream_online INTEGER NOT NULL DEFAULT 0;
