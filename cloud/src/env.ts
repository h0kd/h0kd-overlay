/** Bindings y variables del Worker (ver wrangler.toml). */
export interface Env {
  DB: D1Database;
  CHANNEL_HUB: DurableObjectNamespace;

  // vars
  TWITCH_CLIENT_ID: string;
  PUBLIC_ORIGIN: string;
  /** Beta cerrado: logins separados por coma. Vacía = cualquiera puede darse de alta. */
  ALLOWED_CHANNELS?: string;

  // secrets
  TWITCH_CLIENT_SECRET: string;
  SESSION_SECRET: string;
  TOKEN_ENC_KEY: string;
  EVENTSUB_SECRET: string;
}

/** Nombre de la cookie de sesión. */
export const SESSION_COOKIE = 'vr_session';
/** Cookie efímera que ata el `state` de OAuth al navegador que lo inició. */
export const OAUTH_COOKIE = 'vr_oauth';

export const SESSION_TTL_SECONDS = 60 * 60 * 24 * 30; // 30 días
export const OAUTH_TTL_SECONDS = 60 * 10;             // 10 minutos
export const PAIRING_TTL_SECONDS = 60 * 10;           // 10 minutos

/**
 * Gracia antes de limpiar la cola cuando el stream se va offline. Una microcaída
 * de internet manda `stream.offline` y vuelve enseguida; vaciarle la cola al
 * streamer por eso sería peor que esperar un minuto.
 */
export const OFFLINE_GRACE_SECONDS = 45;
