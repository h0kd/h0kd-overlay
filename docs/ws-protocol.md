# Protocolo WebSocket — Durable Object ↔ Agente

Contrato de mensajes entre el **Durable Object** (uno por canal, en Cloudflare) y el
**agente local** (h0kd-overlay, rama `experimental/video-requests`).

Este documento es la juntura entre las dos mitades del sistema y se valida **antes** de
construir cualquiera de las dos. Los tipos se declaran en TypeScript porque el Worker es
TS; el agente los implementa con `serde` (ver §9 para el mapeo a Rust).

- **Versión del protocolo:** `1`
- **Estado del documento:** implementado de los dos lados (Worker/DO en `cloud/`,
  agente en `src-tauri/src/video_requests.rs`) y probado en vivo con las cinco
  plataformas. Lo único del contrato que no se ejercitó a mano es `clear` por
  `stream_offline`, que solo dispara EventSub.

---

## 1. Transporte y conexión

La conexión la abre **siempre el agente** (saliente), de modo que atraviesa CGNAT sin
abrir puertos:

```
wss://<worker-host>/agent/ws
Authorization: Bearer <agent_token>
Sec-WebSocket-Protocol: h0kd-vr.1
```

- El token va en **header**, nunca en query string (las query strings terminan en logs).
  El agente es Rust (`tokio-tungstenite`), así que puede setear headers; esta ruta no se
  abre nunca desde un browser.
- El `channel_id` **no viaja en la conexión**: el Worker lo deriva del `agent_token` y
  enruta al DO correspondiente. Nada de identidad viene del cliente.
- El subprotocolo declara la versión. Si el DO no la soporta, cierra con `4426`
  (ver §8) y el agente muestra "actualizá la app" en vez de reintentar en loop.
- Un solo agente conectado por canal. Si llega una segunda conexión válida, el DO cierra
  **la anterior** con `4409` (el streamer reinició la app; la conexión vieja es la zombi).

### Emparejamiento (obtención del `agent_token`)

Fuera de banda, una sola vez:

1. El broadcaster entra a `/admin` (login Twitch en el Worker) y pide un **código de
   emparejamiento**: 8 caracteres, TTL 10 minutos, un solo uso.
2. En el agente pega el código y pulsa "Emparejar".
3. El agente hace `POST /agent/pair` con `{ code }` → el Worker responde
   `{ agent_token, channel_id, worker_host }`.
4. El agente persiste eso en `data_dir/video-requests/pairing.json` (mismo patrón que
   `twitch.json`, gitignorado, permisos de usuario).

El `agent_token` es opaco, de larga duración y revocable desde `/admin`. Un token revocado
provoca cierre `4401` en la siguiente conexión.

---

## 2. Envoltura común

Todo mensaje es un objeto JSON UTF-8, máximo **64 KiB**. Los que excedan se descartan y
generan un `error` con `code: "message_too_large"`.

```ts
interface Envelope<T extends string, P> {
  v: 1;            // versión del protocolo
  type: T;         // discriminante
  id: string;      // UUID v4 de ESTE mensaje
  ref?: string;    // `id` del mensaje al que responde (correlación)
  ts: number;      // epoch ms del emisor (informativo; nunca para lógica)
  payload: P;
}
```

Reglas que aplican a ambos lados:

- **Ignorar campos desconocidos.** Es lo que permite agregar campos sin romper versiones.
- **Ignorar `type` desconocidos** en silencio (sin cerrar la conexión), salvo que el
  mensaje traiga `ref`, en cuyo caso se responde `error` con `unsupported_type`.
- **Nunca confiar en `ts` del otro lado** para decidir nada: los relojes se van.
- Toda operación sobre un ítem es **idempotente por `item_id`**. Tras una reconexión los
  mensajes se repiten; procesar dos veces no debe duplicar descargas ni reproducciones.

---

## 3. Mensajes DO → Agente

### `hello`
Primer mensaje tras el upgrade. Da el contexto que el agente necesita sin pedir nada.

```ts
type Hello = Envelope<'hello', {
  channel_id: string;
  channel_login: string;
  protocol_version: 1;
  server_time: number;
  settings: ChannelSettings;
  stream_online: boolean;
}>;

interface ChannelSettings {
  submissions_open: boolean;
  max_duration_seconds: number;   // 30 en el MVP
  max_resolution: '720' | '1080';
  max_filesize_mb: number;        // corta yt-dlp con --max-filesize
  playback_gap_seconds: number;   // 5
}
```

### `metadata.request`
Pide metadata **sin descargar**. Se dispara al enviar un link.

```ts
type MetadataRequest = Envelope<'metadata.request', {
  item_id: string;                // UUID, generado por el Worker
  source_url: string;             // ya validado contra la allowlist
  platform: 'instagram' | 'twitch' | 'youtube';
}>;
```

> El agente **revalida el dominio igual** antes de invocar yt-dlp. La allowlist del Worker
> es la primera defensa, no la única: el DO no es una fuente confiable de URLs.

### `download.request`
Solo llega **después de la aprobación de un mod**. Nada se descarga sin esto.

```ts
type DownloadRequest = Envelope<'download.request', {
  item_id: string;
  source_url: string;
  platform: 'instagram' | 'twitch' | 'youtube';
  max_resolution: '720' | '1080';
  approved_by: string;            // login del mod (auditoría / UI del agente)
}>;
```

### `cancel`
Cancela lo que sea que esté pasando con un ítem: mata el proceso hijo si corre, borra
archivos parciales, y saca el ítem de la cola local si estaba `ready`.

```ts
type Cancel = Envelope<'cancel', {
  item_id: string;
  reason: 'mod_rejected' | 'submitter_deleted' | 'admin' | 'expired';
}>;
```

### `clear`
Vacía la cola local y borra los archivos. Lo dispara el fin de stream (tras la gracia de
30–60 s que aplica el Worker, no el agente).

```ts
type Clear = Envelope<'clear', {
  reason: 'stream_offline' | 'admin';
  keep_now_playing: boolean;      // true = dejá terminar el que está en pantalla
}>;
```

### `settings.update`

```ts
type SettingsUpdate = Envelope<'settings.update', { settings: ChannelSettings }>;
```

### `resync`
Se envía después de cada reconexión, antes que cualquier otro `*.request`. Es la vista
autoritativa de D1: **el agente descarta su cola local y adopta esta**.

```ts
type Resync = Envelope<'resync', {
  items: Array<{
    item_id: string;
    source_url: string;
    platform: 'instagram' | 'twitch' | 'youtube';
    status: 'pending_review' | 'approved' | 'downloading' | 'ready';
    position: number;             // orden FIFO autoritativo
  }>;
}>;
```

---

## 4. Mensajes Agente → DO

### `agent.ready`
Primer mensaje del agente, inmediatamente después de recibir `hello`.

```ts
type AgentReady = Envelope<'agent.ready', {
  agent_version: string;          // versión de la app (ej. "0.3.0-exp")
  ytdlp_version: string | null;   // null = binario ausente
  ffmpeg_version: string | null;
  cookies: CookieStatus;
  encoder: 'h264_nvenc' | 'libx264';
  overlay_connected: boolean;     // ¿hay un Browser Source de OBS enganchado?
}>;

interface CookieStatus {
  present: boolean;
  // 'ok' se degrada a 'expired' recién cuando una extracción falla por login:
  // el archivo no dice si sirve, solo lo dice el uso.
  state: 'ok' | 'expired' | 'missing';
  last_ok_at: number | null;
  last_error_at: number | null;
}
```

### `metadata.result`

```ts
type MetadataResult = Envelope<'metadata.result', {
  item_id: string;
  ok: boolean;
  title?: string;
  duration_seconds?: number;
  thumbnail_url?: string;
  uploader?: string;
  error?: ErrorDetail;            // presente si ok === false
}>;                               // ref = id del metadata.request
```

El DO aplica la regla de duración (`> max_duration_seconds` → `rejected_auto`). El agente
**reporta** la duración, no decide el estado: la máquina de estados vive en la nube.

### `download.progress`
Best-effort. Se emite como mucho **una vez por segundo por ítem**; el DO puede
descartarlos sin consecuencia.

```ts
type DownloadProgress = Envelope<'download.progress', {
  item_id: string;
  stage: 'downloading' | 'probing' | 'transcoding';
  percent: number | null;         // null cuando no hay Content-Length
}>;
```

### `download.result`

```ts
type DownloadResult = Envelope<'download.result', {
  item_id: string;
  ok: boolean;
  duration_seconds?: number;      // real, medido por ffprobe tras recodificar
  width?: number;
  height?: number;
  error?: ErrorDetail;
}>;
```

> La ruta local **nunca** viaja a la nube. El archivo vive en
> `data_dir/video-requests/<item_id>.mp4` y el nombre lo genera el agente (UUID), jamás el
> título remoto.

### `playback.started` / `playback.ended`

```ts
type PlaybackStarted = Envelope<'playback.started', { item_id: string }>;

type PlaybackEnded = Envelope<'playback.ended', {
  item_id: string;
  reason: 'ended' | 'error' | 'cancelled' | 'cleared';
  played_seconds: number;
}>;
```

### `status`
Latido con estado agregado. Cada **15 s**, y además ante cualquier cambio de
`cookies.state`, `now_playing` u `overlay_connected`.

```ts
type Status = Envelope<'status', {
  queue_len: number;
  now_playing: string | null;     // item_id o null
  cookies: CookieStatus;
  ytdlp_version: string | null;
  disk_free_mb: number;
  last_error: ErrorDetail | null;
}>;
```

### `error`
Fallo no atado al ciclo de vida de un ítem (o atado, con `item_id`).

```ts
type AgentError = Envelope<'error', {
  item_id?: string;
  detail: ErrorDetail;
}>;
```

---

## 5. Errores

```ts
interface ErrorDetail {
  code: ErrorCode;
  message: string;                // en español, mostrable tal cual al mod
  retryable: boolean;
}

type ErrorCode =
  // Extracción / plataforma
  | 'cookies_expired'      // IG pide login → hay que renovar cookies
  | 'rate_limited'         // IG nos frenó; bajar ritmo
  | 'extractor_failed'     // yt-dlp no pudo; típicamente extractor roto
  | 'unsupported_platform' // no pasó la revalidación de dominio del agente
  | 'not_found'            // borrado o privado
  // Política
  | 'too_long'
  | 'too_large'
  | 'timeout'
  // Pipeline local
  | 'download_failed'
  | 'probe_failed'         // ffprobe no encontró stream de video sano
  | 'transcode_failed'
  | 'disk_full'
  | 'binary_missing'       // falta yt-dlp o ffmpeg
  // Protocolo
  | 'unsupported_type'
  | 'message_too_large'
  | 'cancelled';
```

`cookies_expired` y `rate_limited` son los dos que exigen acción humana: el agente los
muestra en su UI **y** el DO los propaga al panel de mods, para que nadie se quede
mirando una cola congelada sin saber por qué.

---

## 6. Máquina de estados: quién dispara qué

D1 es la fuente de verdad; el agente mantiene una sombra local que el `resync` corrige.

| Transición | Disparada por |
|---|---|
| `submitted → pending_review` | `metadata.result` con `ok:true` y duración dentro del límite |
| `submitted → rejected_auto` | `metadata.result` con `ok:true` y `duration > max` |
| `submitted → failed` | `metadata.result` con `ok:false` |
| `pending_review → approved` | acción de un mod en `/mod` (nube) |
| `pending_review → rejected` | acción de un mod en `/mod` (nube) |
| `approved → downloading` | el DO emite `download.request` |
| `downloading → ready` | `download.result` con `ok:true` |
| `downloading → failed` | `download.result` con `ok:false` |
| `ready → playing` | `playback.started` |
| `playing → played` | `playback.ended` con `reason:"ended"` |
| cualquiera → `cleared` | `clear`, o `cancel` |

El agente nunca inventa un estado: **reporta hechos** (esto se descargó, esto empezó a
sonar) y la nube deriva el estado. Así, si el agente se cae en cualquier punto, el estado
verdadero sigue estando en D1.

---

## 7. Reconexión, modo degradado y buffer

**Backoff del agente:** 1 s → 2 → 4 → 8 → 16 → 30 s (tope), con jitter ±20 %. Se resetea
tras 60 s de conexión estable.

Mientras la nube no responde, el agente:

- **Sigue reproduciendo** todo lo que ya tiene en `ready`. Un corte de internet del
  streamer no debe cortar el stream.
- **Bufferea** los mensajes salientes en una cola acotada de **200**. Al desbordar, tira
  los más viejos de tipo `download.progress` y `status` primero (son descartables); si
  aun así no entra, tira el más viejo y marca `dropped: true` en el próximo `status`.
- **No acepta ítems nuevos** (no hay de dónde: la entrada viene del DO).

Al reconectar: `hello` → `agent.ready` → el agente vacía el buffer → el DO manda `resync`
→ el agente reconcilia:

- Ítem en `resync` que el agente no conoce y está `approved` → lo descarga.
- Ítem en `resync` en estado `ready` cuyo archivo local **no existe** → el agente reporta
  `download.result` con `ok:false, code:"not_found"` y la nube lo vuelve a pedir.
- Archivo local de un `item_id` que **no aparece** en `resync` → se borra (fue rechazado o
  limpiado mientras el agente estaba desconectado).

---

## 8. Códigos de cierre

| Código | Significado | Reacción del agente |
|---|---|---|
| `1000` | Cierre normal | Reconectar con backoff |
| `1001` | El DO se está hibernando | Reconectar de inmediato |
| `4401` | Token inválido o revocado | **No reintentar.** Pedir emparejamiento de nuevo |
| `4409` | Otra conexión tomó el canal | **No reintentar.** Avisar "abriste la app dos veces" |
| `4426` | Versión de protocolo no soportada | **No reintentar.** Avisar "actualizá la app" |
| `4429` | Demasiadas reconexiones | Reintentar con el backoff al tope (30 s) |

Un token que ya era inválido **antes** del handshake no produce un código de
cierre: el upgrade nunca ocurre y el Worker responde **HTTP 401**. El agente lo
trata igual que `4401` (terminal, pedir emparejamiento de nuevo). El `4401` como
código de cierre queda para la revocación en medio de una sesión ya abierta.

Los tres `4401`/`4409`/`4426` son terminales: reintentarlos es ruido para el Worker y una
UI que miente al streamer.

---

## 9. Notas de implementación

**Rust (agente).** Un `enum` por dirección con `#[serde(tag = "type", content = "payload")]`,
y `#[serde(other)]` en una variante `Unknown` para no romper con mensajes futuros. La
envoltura se aplana con `#[serde(flatten)]`. Todo `Deserialize` que falle se loguea y se
descarta: **un mensaje malformado del DO no puede matar el proceso** (el perfil release usa
`panic = "abort"`, así que un `unwrap` acá se lleva puesta la app entera a mitad de stream).

**TypeScript (Worker/DO).** Los mismos tipos, con un type guard por `type` antes de tocar
el payload. Validar con zod en el borde del DO.

**Ping/pong.** Se usan los frames nativos de WebSocket, no mensajes de aplicación. El DO
pinguea cada 30 s; el agente responde pong automáticamente (tungstenite lo hace solo). Si
el agente pasa 90 s sin tráfico, cierra y reconecta: los `status` cada 15 s hacen que ese
silencio solo ocurra si algo está realmente roto.

**Arbitraje de reproducción.** Vive **entero en el agente**; el DO no sabe nada de él. La
regla acordada: los video requests esperan a que no haya nada en pantalla (+ el gap de 5 s),
y los canjes de puntos siguen disparando al instante, sin cola, exactamente como hoy. El
agente se entera de que el overlay quedó libre por los mensajes `videoStarted`/`videoEnded`
que el overlay manda por el WebSocket **local** — un canal distinto de este, documentado
aparte.
