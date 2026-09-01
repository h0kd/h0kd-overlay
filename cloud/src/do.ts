/**
 * ChannelHub — un Durable Object por canal.
 *
 * Es el único punto donde conviven en vivo el agente del streamer y los paneles
 * de los mods. El Worker (stateless, N instancias) habla con él por fetch
 * interno; el agente y los mods, por WebSocket.
 *
 * Usa la Hibernation API (`acceptWebSocket`): los sockets sobreviven a que el DO
 * sea evacuado de memoria, que es lo normal durante un stream de horas con
 * tráfico esporádico. Como consecuencia NO se puede guardar estado en campos de
 * instancia y esperar que siga ahí: lo que tiene que durar va a `ctx.storage`, y
 * lo que distingue a un socket de otro va en sus tags.
 */

import type { Env } from './env.ts';
import { OFFLINE_GRACE_SECONDS } from './env.ts';
import {
  CLOSE,
  envelope,
  parseFromAgent,
  WS_SUBPROTOCOL,
  type ChannelSettings,
  type FromAgent,
  type ResyncItem,
} from './protocol.ts';
import * as q from './queue.ts';

const TAG_AGENT = 'agent';
const TAG_MOD = 'mod';

interface StoredMeta {
  channel_id: string;
  channel_login: string;
}

/** Último estado reportado por el agente, para mostrarlo en /mod. */
interface AgentSnapshot {
  online: boolean;
  agent_version?: string;
  cookies_state?: string;
  now_playing?: string | null;
  queue_len?: number;
  last_error?: string | null;
  updated_at: number;
}

export class ChannelHub implements DurableObject {
  constructor(
    private ctx: DurableObjectState,
    private env: Env,
  ) {}

  // ── Entrada ────────────────────────────────────────────────────────────────

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    // El Worker inyecta la identidad del canal en cada llamada; el DO la
    // persiste la primera vez para poder usarla tras una hibernación.
    const channelId = request.headers.get('x-channel-id');
    const channelLogin = request.headers.get('x-channel-login');
    if (channelId) {
      await this.ctx.storage.put<StoredMeta>('meta', {
        channel_id: channelId,
        channel_login: channelLogin ?? '',
      });
    }

    switch (url.pathname) {
      case '/ws/agent':
        return this.acceptSocket(request, TAG_AGENT);
      case '/ws/mod':
        return this.acceptSocket(request, TAG_MOD);
      case '/internal/queue-changed':
        await this.pushQueueToMods();
        await this.dispatchPending();
        return json({ ok: true });
      case '/internal/decision': {
        const body = (await request.json()) as {
          item_id: string; approved: boolean; by: string; reason?: string | null;
        };
        await this.onDecision(body);
        return json({ ok: true });
      }
      case '/internal/stream': {
        const body = (await request.json()) as { online: boolean };
        await this.onStreamChange(body.online);
        return json({ ok: true });
      }
      case '/internal/settings':
        await this.sendSettings();
        return json({ ok: true });
      case '/internal/state':
        return json(await this.publicState());
      default:
        return new Response('not found', { status: 404 });
    }
  }

  private acceptSocket(request: Request, tag: string): Response {
    if (request.headers.get('Upgrade') !== 'websocket') {
      return new Response('se esperaba un upgrade a websocket', { status: 426 });
    }

    // Si el cliente pide un subprotocolo hay que confirmarlo en el 101, o el
    // handshake es invalido y un cliente estricto lo rechaza. El agente (Rust)
    // lo pide para declarar la version; los mods (navegador) no piden ninguno.
    const offered = (request.headers.get('Sec-WebSocket-Protocol') ?? '')
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    if (offered.length && !offered.includes(WS_SUBPROTOCOL)) {
      return new Response('version de protocolo no soportada', { status: 426 });
    }
    const headers: Record<string, string> = offered.length
      ? { 'Sec-WebSocket-Protocol': WS_SUBPROTOCOL }
      : {};

    const pair = new WebSocketPair();
    const [client, server] = [pair[0], pair[1]];

    if (tag === TAG_AGENT) {
      // Un solo agente por canal. Si llega uno nuevo, el viejo es el zombi: el
      // streamer reinició la app y la conexión anterior todavía no murió.
      for (const old of this.ctx.getWebSockets(TAG_AGENT)) {
        try {
          old.close(CLOSE.SUPERSEDED, 'otra conexion tomo el canal');
        } catch {
          /* ya estaba muerto */
        }
      }
    }

    this.ctx.acceptWebSocket(server, [tag]);
    // El saludo se manda fuera del camino de respuesta para no demorar el 101.
    this.ctx.waitUntil(tag === TAG_AGENT ? this.greetAgent(server) : this.greetMod(server));
    return new Response(null, { status: 101, webSocket: client, headers });
  }

  // ── Saludos ────────────────────────────────────────────────────────────────

  private async greetAgent(ws: WebSocket): Promise<void> {
    const meta = await this.meta();
    if (!meta) return;
    const settings = await q.getSettings(this.env.DB, meta.channel_id);
    send(ws, envelope('hello', {
      channel_id: meta.channel_id,
      channel_login: meta.channel_login,
      protocol_version: 1 as const,
      server_time: Date.now(),
      settings: toChannelSettings(settings),
      stream_online: settings.submissions_open,
    }));
    await this.sendResync(ws);
    // Lo que entró mientras el agente estaba caído quedó en `submitted`: sin
    // metadata no entra en el resync y nadie más lo va a pedir. Si no se
    // despacha acá, esos pedidos quedan huérfanos para siempre y el viewer ve
    // "leyendo el video…" hasta que termine el stream.
    await this.dispatchPending();
    await this.setAgentSnapshot({ online: true, updated_at: Date.now() });
    await this.pushAgentStateToMods();
  }

  private async greetMod(ws: WebSocket): Promise<void> {
    const meta = await this.meta();
    if (!meta) return;
    send(ws, { type: 'queue', ...(await this.modSnapshot(meta.channel_id)) });
    send(ws, { type: 'agent', ...(await this.agentSnapshot()) });
  }

  /**
   * Vista autoritativa de la cola para el agente. Se manda en cada reconexión,
   * antes que cualquier pedido: el agente descarta su cola local y adopta esta.
   */
  private async sendResync(ws: WebSocket): Promise<void> {
    const meta = await this.meta();
    if (!meta) return;
    const rows = await q.listByStatus(this.env.DB, meta.channel_id, [
      'pending_review',
      'approved',
      'downloading',
      'ready',
    ]);
    const items: ResyncItem[] = rows.map((r, i) => ({
      item_id: r.id,
      source_url: r.source_url,
      platform: r.platform,
      status: r.status as ResyncItem['status'],
      position: i,
    }));
    send(ws, envelope('resync', { items }));
  }

  // ── Mensajes entrantes ─────────────────────────────────────────────────────

  async webSocketMessage(ws: WebSocket, raw: ArrayBuffer | string): Promise<void> {
    // Los mods no mandan nada: sus acciones van por HTTP al Worker, que es
    // donde vive la comprobación de permisos. Un socket no autoriza nada.
    if (!this.ctx.getTags(ws).includes(TAG_AGENT)) return;
    if (typeof raw !== 'string') return;

    const msg = parseFromAgent(raw);
    if (!msg) return; // malformado: se descarta, nunca tumba el hub

    const meta = await this.meta();
    if (!meta) return;

    try {
      await this.handleAgentMessage(meta.channel_id, msg);
    } catch (err) {
      console.error('[ChannelHub] fallo procesando', msg.type, err);
    }
  }

  private async handleAgentMessage(channelId: string, msg: FromAgent): Promise<void> {
    const db = this.env.DB;

    switch (msg.type) {
      case 'agent.ready': {
        await this.setAgentSnapshot({
          online: true,
          agent_version: msg.payload.agent_version,
          cookies_state: msg.payload.cookies.state,
          updated_at: Date.now(),
        });
        await this.pushAgentStateToMods();
        break;
      }

      case 'metadata.result': {
        const item = await q.getItem(db, channelId, msg.payload.item_id);
        if (!item || item.status !== 'submitted') break; // idempotencia

        if (!msg.payload.ok) {
          await q.setStatus(db, channelId, item.id, 'failed', {
            error: msg.payload.error?.message ?? 'No se pudo leer el video.',
          });
          break;
        }
        await q.setMetadata(db, channelId, item.id, {
          title: msg.payload.title,
          duration_seconds: msg.payload.duration_seconds,
          thumbnail_url: msg.payload.thumbnail_url,
        });
        const settings = await q.getSettings(db, channelId);
        const dur = msg.payload.duration_seconds;

        // yt-dlp NO devuelve duración para los Reels de Instagram (verificado
        // contra reels reales). O sea que el rechazo automático por duración no
        // se puede aplicar justo en la plataforma prioritaria.
        //
        // No es un agujero: el límite duro lo impone ffmpeg al recodificar
        // (`-t <max>`), así que nada más largo que el máximo llega nunca al
        // overlay, se haya sabido la duración o no. Lo que se pierde es poder
        // descartarlo ANTES de descargarlo. Cuando no se sabe, decide el mod,
        // que puede ver el original.
        if (dur !== undefined && dur > settings.max_duration_seconds) {
          await q.setStatus(db, channelId, item.id, 'rejected_auto', {
            error: `Dura ${Math.round(dur)}s; el máximo es ${settings.max_duration_seconds}s.`,
          });
        } else {
          await q.setStatus(db, channelId, item.id, 'pending_review');
        }
        await this.pushQueueToMods();
        break;
      }

      case 'download.result': {
        const item = await q.getItem(db, channelId, msg.payload.item_id);
        if (!item || item.status !== 'downloading') break;
        if (msg.payload.ok) {
          await q.setStatus(db, channelId, item.id, 'ready');
        } else {
          await q.setStatus(db, channelId, item.id, 'failed', {
            error: msg.payload.error?.message ?? 'Falló la descarga.',
          });
        }
        await this.pushQueueToMods();
        break;
      }

      case 'playback.started':
        await q.setStatus(db, channelId, msg.payload.item_id, 'playing');
        await this.pushQueueToMods();
        break;

      case 'playback.ended':
        await q.setStatus(
          db,
          channelId,
          msg.payload.item_id,
          msg.payload.reason === 'ended' ? 'played' : 'cleared',
        );
        await this.pushQueueToMods();
        break;

      case 'status':
        await this.setAgentSnapshot({
          online: true,
          cookies_state: msg.payload.cookies.state,
          now_playing: msg.payload.now_playing,
          queue_len: msg.payload.queue_len,
          last_error: msg.payload.last_error?.message ?? null,
          updated_at: Date.now(),
        });
        await this.pushAgentStateToMods();
        break;

      case 'error':
        if (msg.payload.item_id) {
          await q.setStatus(db, channelId, msg.payload.item_id, 'failed', {
            error: msg.payload.detail.message,
          });
          await this.pushQueueToMods();
        }
        await this.setAgentSnapshot({
          online: true,
          last_error: msg.payload.detail.message,
          updated_at: Date.now(),
        });
        await this.pushAgentStateToMods();
        break;

      case 'download.progress':
        // Best-effort: no toca D1. Solo interesa en vivo, en el panel de mods.
        this.broadcastMods({
          type: 'progress',
          item_id: msg.payload.item_id,
          stage: msg.payload.stage,
          percent: msg.payload.percent,
        });
        break;
    }
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    if (this.ctx.getTags(ws).includes(TAG_AGENT)) {
      await this.setAgentSnapshot({ online: false, updated_at: Date.now() });
      await this.pushAgentStateToMods();
    }
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    await this.webSocketClose(ws);
  }

  // ── Acciones que vienen del Worker ─────────────────────────────────────────

  /** Pide metadata de todo lo que entró y todavía no se consultó. */
  private async dispatchPending(): Promise<void> {
    const meta = await this.meta();
    const agent = this.agentSocket();
    if (!meta || !agent) return;
    for (const item of await q.listByStatus(this.env.DB, meta.channel_id, ['submitted'])) {
      send(agent, envelope('metadata.request', {
        item_id: item.id,
        source_url: item.source_url,
        platform: item.platform,
      }));
    }
  }

  private async onDecision(
    body: { item_id: string; approved: boolean; by: string; reason?: string | null },
  ): Promise<void> {
    const meta = await this.meta();
    if (!meta) return;
    const item = await q.getItem(this.env.DB, meta.channel_id, body.item_id);
    if (!item || item.status !== 'pending_review') return;

    const agent = this.agentSocket();
    if (body.approved) {
      const settings = await q.getSettings(this.env.DB, meta.channel_id);
      // `approved` primero: si el agente está caído, el resync se lo lleva al
      // reconectar en vez de perderse.
      await q.setStatus(this.env.DB, meta.channel_id, item.id, 'approved', { decided_by: body.by });
      if (agent) {
        await q.setStatus(this.env.DB, meta.channel_id, item.id, 'downloading');
        send(agent, envelope('download.request', {
          item_id: item.id,
          source_url: item.source_url,
          platform: item.platform,
          max_resolution: settings.max_resolution,
          approved_by: body.by,
        }));
      }
    } else {
      // Lo rechazado NUNCA se descarga: no sale ningún download.request.
      // El motivo se guarda con el rechazo y viaja al viewer: "rechazado" a
      // secas es lo que hace que la gente vuelva a mandar lo mismo.
      await q.setStatus(this.env.DB, meta.channel_id, item.id, 'rejected', {
        decided_by: body.by,
        decided_reason: body.reason ?? null,
      });
      if (agent) send(agent, envelope('cancel', { item_id: item.id, reason: 'mod_rejected' }));
    }
    await this.pushQueueToMods();
  }

  /**
   * stream.online abre los envíos al toque. stream.offline no cierra nada de
   * inmediato: arma una alarma con la gracia, porque una microcaída manda
   * offline y vuelve, y vaciar la cola por eso sería peor que esperar.
   */
  private async onStreamChange(online: boolean): Promise<void> {
    const meta = await this.meta();
    if (!meta) return;

    if (online) {
      await this.ctx.storage.deleteAlarm();
      await q.setSubmissionsOpen(this.env.DB, meta.channel_id, true);
      await this.sendSettings();
      await this.pushQueueToMods();
      return;
    }

    await q.setSubmissionsOpen(this.env.DB, meta.channel_id, false);
    await this.sendSettings();
    await this.ctx.storage.setAlarm(Date.now() + OFFLINE_GRACE_SECONDS * 1000);
  }

  /** Se cumplió la gracia y el stream sigue offline: limpiar de verdad. */
  async alarm(): Promise<void> {
    const meta = await this.meta();
    if (!meta) return;
    const settings = await q.getSettings(this.env.DB, meta.channel_id);
    if (settings.submissions_open) return; // volvió online durante la gracia

    await q.clearQueue(this.env.DB, meta.channel_id);
    const agent = this.agentSocket();
    if (agent) {
      send(agent, envelope('clear', { reason: 'stream_offline', keep_now_playing: true }));
    }
    await this.pushQueueToMods();
  }

  private async sendSettings(): Promise<void> {
    const meta = await this.meta();
    const agent = this.agentSocket();
    if (!meta || !agent) return;
    const settings = await q.getSettings(this.env.DB, meta.channel_id);
    send(agent, envelope('settings.update', { settings: toChannelSettings(settings) }));
  }

  // ── Estado hacia los mods ──────────────────────────────────────────────────

  /**
   * Lo que ve el panel de mods: la cola revisable, MÁS cuántos pedidos están
   * esperando metadata del agente.
   *
   * Un ítem en `submitted` no entra en la lista porque no tiene título, ni
   * miniatura, ni duración: no hay nada que revisar todavía. Pero si el agente
   * está caído se acumulan ahí, y sin este contador el panel diría "no hay nada
   * en la cola" mientras la gente manda links al vacío.
   */
  private async modSnapshot(channelId: string) {
    const waiting = await q.listByStatus(this.env.DB, channelId, ['submitted']);
    const rows = await q.listByStatus(this.env.DB, channelId, [
      'pending_review',
      'approved',
      'downloading',
      'ready',
      'playing',
      'failed',
    ]);
    const fotos = await q.picsFor(
      this.env.DB,
      rows.flatMap((r) => [r.submitter_login, r.decided_by]),
    );
    const items = rows.map((r) => ({
      id: r.id,
      status: r.status,
      title: r.title,
      thumbnail_url: r.thumbnail_url,
      duration_seconds: r.duration_seconds,
      source_url: r.source_url,
      platform: r.platform,
      submitter_login: r.submitter_login,
      submitter_pic: fotos.get(r.submitter_login.toLowerCase()) ?? null,
      error: r.error,
      created_at: r.created_at,
      decided_by: r.decided_by,
      decided_pic: fotos.get((r.decided_by ?? '').toLowerCase()) ?? null,
    }));
    return { items, waiting: waiting.length };
  }

  private async pushQueueToMods(): Promise<void> {
    const meta = await this.meta();
    if (!meta) return;
    this.broadcastMods({ type: 'queue', ...(await this.modSnapshot(meta.channel_id)) });
  }

  private async pushAgentStateToMods(): Promise<void> {
    this.broadcastMods({ type: 'agent', ...(await this.agentSnapshot()) });
  }

  private broadcastMods(payload: unknown): void {
    const text = JSON.stringify(payload);
    for (const ws of this.ctx.getWebSockets(TAG_MOD)) {
      try {
        ws.send(text);
      } catch {
        /* socket muerto; se limpia solo en webSocketClose */
      }
    }
  }

  // ── Helpers ────────────────────────────────────────────────────────────────

  private agentSocket(): WebSocket | null {
    return this.ctx.getWebSockets(TAG_AGENT)[0] ?? null;
  }

  private async meta(): Promise<StoredMeta | null> {
    return (await this.ctx.storage.get<StoredMeta>('meta')) ?? null;
  }

  private async agentSnapshot(): Promise<AgentSnapshot> {
    const snap = await this.ctx.storage.get<AgentSnapshot>('agent');
    const online = this.agentSocket() !== null;
    return snap ? { ...snap, online } : { online, updated_at: 0 };
  }

  private async setAgentSnapshot(patch: Partial<AgentSnapshot>): Promise<void> {
    const prev = (await this.ctx.storage.get<AgentSnapshot>('agent')) ?? { online: false, updated_at: 0 };
    await this.ctx.storage.put<AgentSnapshot>('agent', { ...prev, ...patch });
  }

  private async publicState() {
    const meta = await this.meta();
    return {
      channel_id: meta?.channel_id ?? null,
      agent: await this.agentSnapshot(),
      mods_connected: this.ctx.getWebSockets(TAG_MOD).length,
    };
  }
}

function toChannelSettings(s: q.FullSettings): ChannelSettings {
  return {
    submissions_open: s.submissions_open,
    max_duration_seconds: s.max_duration_seconds,
    max_resolution: s.max_resolution,
    max_filesize_mb: s.max_filesize_mb,
    playback_gap_seconds: s.playback_gap_seconds,
  };
}

function send(ws: WebSocket, msg: unknown): void {
  try {
    ws.send(JSON.stringify(msg));
  } catch {
    /* el socket se está cerrando; el close handler se encarga */
  }
}

function json(data: unknown): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'content-type': 'application/json' },
  });
}
