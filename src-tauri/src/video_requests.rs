//! Integración de Video Requests con la app.
//!
//! Todo lo pesado (descargar, validar, recodificar) vive en el crate
//! `video-requests-core`, que no sabe nada de Tauri. Acá está solo la juntura:
//! el WebSocket saliente al Durable Object del canal, el emparejamiento, y el
//! arbitraje de reproducción contra el overlay.
//!
//! Sigue el mismo patrón que `twitch.rs` — un worker en segundo plano con un
//! canal de comandos y un estado compartido que la UI lee por polling — porque
//! ya está probado en esta app y no había motivo para inventar otro.
//!
//! **Con el flag apagado nada de este módulo se construye.** `lib.rs` ni
//! siquiera lanza el worker.

use crate::logln;
use crate::AppState;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use video_requests_core as core;

/// Origen del Worker. NO tiene valor por defecto a propósito: hardcodear un
/// despliegue concreto en una app que se distribuye ataría a todos los usuarios
/// al servidor de una sola persona. Cada quien pone el suyo en config.json,
/// bajo `videoRequests.workerOrigin`.
const WORKER_ORIGIN_KEY: &str = "videoRequests.workerOrigin";

const WS_SUBPROTOCOL: &str = "h0kd-vr.1";
const PROTOCOL_VERSION: u64 = 1;

/// Cada cuánto se manda el latido de estado al hub.
const STATUS_EVERY: Duration = Duration::from_secs(15);
/// Cada cuánto se revisa si se puede reproducir el siguiente.
const TICK: Duration = Duration::from_millis(400);

// ── Estado compartido con la UI ──────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct VrStatus {
    /// "unpaired" | "connecting" | "connected" | "error"
    pub state: String,
    #[serde(rename = "channelLogin")]
    pub channel_login: Option<String>,
    #[serde(rename = "queueLen")]
    pub queue_len: usize,
    #[serde(rename = "nowPlaying")]
    pub now_playing: Option<String>,
    #[serde(rename = "cookiesState")]
    pub cookies_state: String,
    #[serde(rename = "binariesOk")]
    pub binaries_ok: bool,
    #[serde(rename = "ytdlpVersion")]
    pub ytdlp_version: Option<String>,
    /// Verdadero mientras se bajan yt-dlp o ffmpeg. La UI lo mira para dejar el
    /// botón en "Bajando…": la descarga ya no bloquea a quien la pidió, así que
    /// el comando vuelve enseguida y sin esto parecería que ya terminó.
    pub installing: bool,
    pub error: Option<String>,
}

impl Default for VrStatus {
    fn default() -> Self {
        VrStatus {
            state: "unpaired".into(),
            channel_login: None,
            queue_len: 0,
            now_playing: None,
            cookies_state: "missing".into(),
            binaries_ok: false,
            ytdlp_version: None,
            installing: false,
            error: None,
        }
    }
}

pub type SharedVrStatus = Arc<Mutex<VrStatus>>;

fn update(shared: &SharedVrStatus, f: impl FnOnce(&mut VrStatus)) {
    if let Ok(mut s) = shared.lock() {
        f(&mut s);
    }
}

// ── Comandos desde la UI y desde el overlay ──────────────────────────────────

pub enum VrCmd {
    Pair(String),
    Unpair,
    /// El overlay avisó que empezó o terminó de mostrar algo.
    Overlay(OverlayEvent),
    InstallBinaries,
    UpdateYtdlp,
    /// Volver a conectar después de un cierre terminal (p. ej. 4409, otra
    /// instancia tomó el canal y ya se cerró). Sin esto había que reiniciar.
    Reconnect,
}

#[derive(Debug, Clone)]
pub enum OverlayEvent {
    /// Hay (o no) algún video en pantalla, del origen que sea.
    Busy(bool),
    RequestStarted(String),
    RequestEnded {
        item_id: String,
        seconds: f64,
        /// "ended" si el video terminó solo; "error" si no se pudo reproducir.
        reason: String,
    },
}

// ── Emparejamiento persistido ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default, Clone)]
struct Pairing {
    #[serde(default)]
    agent_token: String,
    #[serde(default)]
    channel_id: String,
    #[serde(default)]
    channel_login: String,
    #[serde(default)]
    worker_origin: String,
}

fn pairing_path(data_dir: &Path) -> PathBuf {
    data_dir.join("video-requests").join("pairing.json")
}

fn load_pairing(data_dir: &Path) -> Pairing {
    match std::fs::read_to_string(pairing_path(data_dir)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Pairing::default(),
    }
}

fn save_pairing(data_dir: &Path, p: &Pairing) {
    let path = pairing_path(data_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(p) {
        let _ = std::fs::write(path, s);
    }
}

/// Origen del Worker: el de config.json si está, si no el por defecto.
fn worker_origin(data_dir: &Path) -> String {
    let cfg: Value = std::fs::read_to_string(data_dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    cfg["videoRequests"]["workerOrigin"]
        .as_str()
        .map(str::trim)
        .filter(|s| s.starts_with("https://"))
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string()
}

/// Carpeta donde viven los mp4 ya listos para reproducir.
pub fn media_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("video-requests").join("media")
}

// ── La cola local ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct ReadyItem {
    item_id: String,
    file: String,
    title: String,
    submitter: String,
}

/// Estado de reproducción del agente.
///
/// El arbitraje acordado: los video requests esperan a que la pantalla esté
/// libre; los canjes de puntos NUNCA esperan y siguen disparando al instante,
/// exactamente igual que en la versión estable. Por eso acá solo hay una cola
/// de requests y un booleano que dice si hay algo en pantalla — el camino de
/// los canjes no pasa por este módulo en ningún momento.
struct Playback {
    queue: VecDeque<ReadyItem>,
    overlay_busy: bool,
    now_playing: Option<String>,
    free_since: Option<Instant>,
    gap: Duration,
}

impl Playback {
    fn new() -> Self {
        Playback {
            queue: VecDeque::new(),
            overlay_busy: false,
            now_playing: None,
            free_since: Some(Instant::now()),
            gap: Duration::from_secs(5),
        }
    }

    fn set_busy(&mut self, busy: bool) {
        self.overlay_busy = busy;
        if busy {
            self.free_since = None;
        } else if self.free_since.is_none() {
            self.free_since = Some(Instant::now());
        }
    }

    /// ¿Se puede largar el siguiente? Requiere pantalla libre, el gap cumplido
    /// y que no haya nada nuestro reproduciéndose.
    fn next_to_play(&mut self) -> Option<ReadyItem> {
        if self.overlay_busy || self.now_playing.is_some() || self.queue.is_empty() {
            return None;
        }
        let waited = self.free_since.map(|t| t.elapsed()).unwrap_or_default();
        if waited < self.gap {
            return None;
        }
        self.queue.pop_front()
    }
}

/// Los límites que manda el canal.
///
/// El DO los envía en `hello` y en cada `settings.update`. Hasta que llegue el
/// primero valen los de fábrica, que son los mismos que trae el pipeline.
#[derive(Clone)]
struct ChannelLimits {
    max_duration_seconds: u32,
    max_filesize_mb: u32,
    /// Lado corto de la salida: 720 o 1080.
    max_resolution: u32,
}

impl Default for ChannelLimits {
    fn default() -> Self {
        let d = core::PipelineConfig::default();
        ChannelLimits {
            max_duration_seconds: d.max_duration_seconds,
            max_filesize_mb: d.max_filesize_mb,
            max_resolution: d.max_short_side,
        }
    }
}

impl ChannelLimits {
    /// Adopta lo que venga en `payload.settings`. Lo que falte o venga con un
    /// valor imposible se deja como estaba: el agente no confia en que del otro
    /// lado siempre haya un DO sano, y quedarse con el límite anterior es mejor
    /// que recortar todos los videos a cero.
    fn apply(&mut self, s: &Value) {
        if let Some(v) = s["max_duration_seconds"].as_u64() {
            self.max_duration_seconds = v.clamp(5, 120) as u32;
        }
        if let Some(v) = s["max_filesize_mb"].as_u64() {
            self.max_filesize_mb = v.clamp(1, 500) as u32;
        }
        if let Some(v) = s["max_resolution"].as_str() {
            self.max_resolution = if v == "1080" { 1080 } else { 720 };
        }
    }
}

// ── Worker ───────────────────────────────────────────────────────────────────

pub async fn worker_loop(state: AppState, mut rx: mpsc::Receiver<VrCmd>) {
    let data_dir = state.data_dir.clone();
    let shared = match state.video_requests.clone() {
        Some(s) => s,
        None => return, // flag apagado: no debería llegarse acá
    };

    let _ = std::fs::create_dir_all(media_dir(&data_dir));

    // Diagnóstico inicial, para que la UI diga algo útil desde el arranque.
    refresh_environment(&data_dir, &shared).await;

    let mut pairing = load_pairing(&data_dir);
    let mut playback = Playback::new();
    let mut backoff = Duration::from_secs(1);
    let mut status_tick = tokio::time::interval(STATUS_EVERY);
    let mut play_tick = tokio::time::interval(TICK);

    loop {
        if pairing.agent_token.is_empty() {
            update(&shared, |s| {
                s.state = "unpaired".into();
                s.channel_login = None;
            });
            // Sin emparejar no hay a dónde conectarse: se espera un comando.
            match rx.recv().await {
                Some(cmd) => {
                    handle_offline_cmd(cmd, &data_dir, &shared, &mut pairing).await;
                    continue;
                }
                None => return,
            }
        }

        update(&shared, |s| {
            s.state = "connecting".into();
            s.channel_login = Some(pairing.channel_login.clone());
            s.error = None;
        });

        let arranco = Instant::now();
        match session(&state, &data_dir, &shared, &pairing, &mut playback, &mut rx).await {
            SessionEnd::Terminal(msg) => {
                logln!("[VideoRequests] sesión cortada, no se reintenta: {msg}");
                update(&shared, |s| {
                    s.state = "error".into();
                    s.error = Some(msg);
                });
                // Terminal = no reintentar. Se espera un comando del usuario.
                match rx.recv().await {
                    Some(cmd) => {
                        handle_offline_cmd(cmd, &data_dir, &shared, &mut pairing).await;
                        continue;
                    }
                    None => return,
                }
            }
            SessionEnd::Unpaired => {
                pairing = Pairing::default();
                continue;
            }
            SessionEnd::Retry(msg) => {
                // Una sesión que se sostuvo un rato no es una caída en cadena:
                // el backoff vuelve a cero. Sin esto solo crecía, así que tras
                // unos cortes seguidos quedaba clavado en 30 s PARA SIEMPRE, y
                // una microcaída en medio del stream costaba medio minuto de
                // cola frenada por algo que había pasado tres horas antes.
                if arranco.elapsed() >= Duration::from_secs(60) {
                    backoff = Duration::from_secs(1);
                }
                logln!("[VideoRequests] reconecta en {}s: {msg}", backoff.as_secs());
                update(&shared, |s| {
                    s.state = "connecting".into();
                    s.error = Some(msg);
                });
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }

        // Los ticks no se usan fuera de la sesión; se resetean para que el
        // primer tick tras reconectar no dispare inmediatamente.
        status_tick.reset();
        play_tick.reset();
    }
}

enum SessionEnd {
    /// Reintentar con backoff.
    Retry(String),
    /// No reintentar: token revocado, doble instancia o versión vieja.
    Terminal(String),
    /// El usuario desemparejó.
    Unpaired,
}

async fn handle_offline_cmd(
    cmd: VrCmd,
    data_dir: &Path,
    shared: &SharedVrStatus,
    pairing: &mut Pairing,
) {
    match cmd {
        VrCmd::Pair(code) => match pair(data_dir, &code).await {
            Ok(p) => {
                save_pairing(data_dir, &p);
                *pairing = p;
                update(shared, |s| s.error = None);
            }
            Err(e) => update(shared, |s| s.error = Some(e)),
        },
        VrCmd::Unpair => {
            *pairing = Pairing::default();
            let _ = std::fs::remove_file(pairing_path(data_dir));
            update(shared, |s| {
                s.state = "unpaired".into();
                s.channel_login = None;
                s.error = None;
            });
        }
        VrCmd::InstallBinaries => spawn_binaries_job(data_dir, shared, BinariesJob::Install),
        VrCmd::UpdateYtdlp => spawn_binaries_job(data_dir, shared, BinariesJob::UpdateYtdlp),
        // No hay nada que hacer acá: con volver al loop, si hay token, se
        // intenta la sesión de nuevo. Se limpia el error para que la UI no
        // muestre el motivo viejo mientras conecta.
        VrCmd::Reconnect => update(shared, |s| s.error = None),
        VrCmd::Overlay(_) => {}
    }
}

/// Canjea el código de `/admin` por un token de agente.
async fn pair(data_dir: &Path, code: &str) -> Result<Pairing, String> {
    let origin = worker_origin(data_dir);
    if origin.is_empty() {
        return Err(format!(
            "Falta la dirección del servidor. Poné tu Worker en config.json, en {WORKER_ORIGIN_KEY}."
        ));
    }
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{origin}/agent/pair"))
        .json(&json!({ "code": code.trim().to_uppercase() }))
        .send()
        .await
        .map_err(|e| format!("No se pudo contactar al servidor: {e}"))?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(body["error"].as_str().unwrap_or("Código inválido o vencido.").to_string());
    }

    let token = body["agent_token"].as_str().unwrap_or_default();
    if token.is_empty() {
        return Err("El servidor no devolvió un token.".into());
    }
    Ok(Pairing {
        agent_token: token.to_string(),
        channel_id: body["channel_id"].as_str().unwrap_or_default().to_string(),
        channel_login: body["channel_login"].as_str().unwrap_or_default().to_string(),
        worker_origin: origin,
    })
}

async fn refresh_environment(data_dir: &Path, shared: &SharedVrStatus) {
    let d = core::doctor(data_dir).await;
    update(shared, |s| {
        s.binaries_ok = d.ytdlp_version.is_some() && d.ffmpeg_found && d.ffprobe_found;
        s.ytdlp_version = d.ytdlp_version.clone();
        s.cookies_state = match d.cookies.state {
            core::CookieState::Ok => "ok",
            core::CookieState::Expired => "expired",
            core::CookieState::Missing => "missing",
        }
        .to_string();
    });
}

/// Qué hay que bajar.
enum BinariesJob {
    /// yt-dlp y/o ffmpeg, lo que falte.
    Install,
    /// `yt-dlp -U`.
    UpdateYtdlp,
}

/// Lanza la descarga en una tarea aparte y vuelve en el acto.
///
/// Esto NO se puede `await` desde el bucle de la sesión: ffmpeg son ~110 MB, y
/// mientras baja el agente no leería los mensajes del hub, no mandaría el
/// latido de estado ni largaría el siguiente video. El DO lo daría por muerto
/// y la cola se frenaría en pleno stream por haber apretado un botón.
///
/// El flag `installing` hace de candado: dos clicks no bajan dos veces.
fn spawn_binaries_job(data_dir: &Path, shared: &SharedVrStatus, job: BinariesJob) {
    let mut go = false;
    if let Ok(mut s) = shared.lock() {
        if !s.installing {
            s.installing = true;
            s.error = None;
            go = true;
        }
    }
    if !go {
        return;
    }

    let data_dir = data_dir.to_path_buf();
    let shared = Arc::clone(shared);
    tokio::spawn(async move {
        let outcome = match job {
            BinariesJob::Install => core::binaries::install_missing(&data_dir).await.map(|_| ()),
            BinariesJob::UpdateYtdlp => {
                core::binaries::update_ytdlp(&data_dir).await.map(|_| ())
            }
        };
        if let Err(e) = outcome {
            update(&shared, |s| s.error = Some(e.message));
        }
        refresh_environment(&data_dir, &shared).await;
        update(&shared, |s| s.installing = false);
    });
}

// ── Una sesión de WebSocket ──────────────────────────────────────────────────

async fn session(
    state: &AppState,
    data_dir: &Path,
    shared: &SharedVrStatus,
    pairing: &Pairing,
    playback: &mut Playback,
    rx: &mut mpsc::Receiver<VrCmd>,
) -> SessionEnd {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;

    let origin = if pairing.worker_origin.is_empty() {
        worker_origin(data_dir)
    } else {
        pairing.worker_origin.clone()
    };
    let ws_url = format!("{}/agent/ws", origin.replacen("https://", "wss://", 1));

    let mut req = match ws_url.into_client_request() {
        Ok(r) => r,
        Err(e) => return SessionEnd::Terminal(format!("URL del servidor inválida: {e}")),
    };
    // El token va en header, nunca en la query: las query strings terminan en logs.
    match HeaderValue::from_str(&format!("Bearer {}", pairing.agent_token)) {
        Ok(v) => {
            req.headers_mut().insert("Authorization", v);
        }
        Err(_) => return SessionEnd::Terminal("Token de emparejamiento inválido.".into()),
    }
    if let Ok(v) = HeaderValue::from_str(WS_SUBPROTOCOL) {
        req.headers_mut().insert("Sec-WebSocket-Protocol", v);
    }

    let (ws, _resp) = match tokio_tungstenite::connect_async(req).await {
        Ok(ok) => ok,
        Err(e) => {
            let msg = e.to_string();
            // Un 401 en el handshake significa token revocado: reintentar es
            // ruido para el servidor y una UI que le miente al streamer.
            if msg.contains("401") {
                return SessionEnd::Terminal(
                    "El emparejamiento fue revocado. Generá un código nuevo en /admin.".into(),
                );
            }
            return SessionEnd::Retry(format!("Sin conexión con el servidor: {msg}"));
        }
    };
    let (mut tx_ws, mut rx_ws) = ws.split();

    update(shared, |s| {
        s.state = "connected".into();
        s.error = None;
    });
    logln!("[VideoRequests] Conectado al canal '{}'.", pairing.channel_login);

    let media = media_dir(data_dir);
    let mut limits = ChannelLimits::default();
    let mut status_tick = tokio::time::interval(STATUS_EVERY);
    let mut play_tick = tokio::time::interval(TICK);
    let _ = send_ready(state, &mut tx_ws, data_dir).await;

    loop {
        tokio::select! {
            // ── Mensajes del hub ──
            incoming = rx_ws.next() => match incoming {
                Some(Ok(Message::Text(txt))) => {
                    let v: Value = serde_json::from_str(&txt).unwrap_or(Value::Null);
                    // Los binarios se resuelven en cada mensaje y no una vez por
                    // sesión: si alguien aprieta "Instalar lo que falte" con la
                    // conexión abierta, lo recién bajado tiene que verse sin
                    // reiniciar la app. Son tres `is_file()`, no cuesta nada.
                    let bins = core::binaries::resolve(data_dir);
                    if let Some(end) = handle_hub_message(
                        state, &v, &bins, &media, playback, &mut limits, shared, &mut tx_ws,
                    ).await {
                        return end;
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    let code = frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(0);
                    return match code {
                        4401 => SessionEnd::Terminal(
                            "El emparejamiento fue revocado. Generá un código nuevo en /admin.".into()),
                        4409 => SessionEnd::Terminal(
                            "Otra instancia de la app tomó el canal.".into()),
                        4426 => SessionEnd::Terminal(
                            "El servidor pide una versión más nueva de la app.".into()),
                        _ => SessionEnd::Retry("El servidor cerró la conexión.".into()),
                    };
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return SessionEnd::Retry(format!("Error de conexión: {e}")),
                None => return SessionEnd::Retry("Se cortó la conexión.".into()),
            },

            // ── Comandos de la UI y del overlay ──
            cmd = rx.recv() => match cmd {
                Some(VrCmd::Overlay(ev)) => {
                    apply_overlay_event(ev, &media, playback, &mut tx_ws).await;
                    update(shared, |s| s.now_playing = playback.now_playing.clone());
                }
                Some(VrCmd::Unpair) => {
                    let _ = std::fs::remove_file(pairing_path(data_dir));
                    return SessionEnd::Unpaired;
                }
                Some(VrCmd::Pair(code)) => {
                    // Reemparejar con la sesión abierta: se guarda y se reconecta.
                    match pair(data_dir, &code).await {
                        Ok(p) => { save_pairing(data_dir, &p); return SessionEnd::Retry("Reemparejado.".into()); }
                        Err(e) => update(shared, |s| s.error = Some(e)),
                    }
                }
                Some(VrCmd::InstallBinaries) => {
                    spawn_binaries_job(data_dir, shared, BinariesJob::Install);
                }
                Some(VrCmd::UpdateYtdlp) => {
                    spawn_binaries_job(data_dir, shared, BinariesJob::UpdateYtdlp);
                }
                // Con la sesión abierta, reconectar es cortar y volver a entrar.
                Some(VrCmd::Reconnect) => return SessionEnd::Retry("Reconectando.".into()),
                None => return SessionEnd::Terminal("La app se está cerrando.".into()),
            },

            // ── Arbitraje: ¿se puede largar el siguiente? ──
            _ = play_tick.tick() => {
                // Si el overlay se cae con algo NUESTRO en pantalla, eso no se
                // arregla solo: el `requestEnded` lo manda el overlay, y si no
                // está no lo manda nadie. `now_playing` quedaría puesto para
                // siempre y la cola entera detrás, esperando un final que no va
                // a llegar. Cerrarlo acá es la única salida.
                if state.tx.receiver_count() == 0 {
                    if let Some(item_id) = playback.now_playing.take() {
                        logln!("[VideoRequests] el overlay se fue con {item_id} en pantalla");
                        let msg = envelope(
                            "playback.ended",
                            json!({ "item_id": item_id, "reason": "error", "played_seconds": 0 }),
                        );
                        let _ = tx_ws.send(Message::Text(msg.to_string())).await;
                        playback.set_busy(false);
                        update(shared, |s| s.now_playing = None);
                    }
                }

                if let Some(item) = playback.next_to_play() {
                    playback.now_playing = Some(item.item_id.clone());
                    let msg = json!({
                        "event": { "source": "VideoRequests", "type": "Custom" },
                        "data": {
                            "action": "playRequest",
                            "itemId": item.item_id,
                            "src": format!("video-requests/{}", item.file),
                            "title": item.title,
                            "submitter": item.submitter
                        }
                    });
                    let clients = state.tx.send(msg.to_string()).unwrap_or(0);
                    if clients > 0 {
                        logln!("[VideoRequests] → overlay {}", item.item_id);
                    }
                    if clients == 0 {
                        // Sin overlay conectado no se puede reproducir: vuelve
                        // a la cola en vez de darse por reproducido.
                        playback.now_playing = None;
                        playback.queue.push_front(item);
                    }
                    update(shared, |s| {
                        s.now_playing = playback.now_playing.clone();
                        s.queue_len = playback.queue.len();
                    });
                }
            }

            // ── Latido ──
            _ = status_tick.tick() => {
                let snapshot = shared.lock().map(|s| s.clone()).unwrap_or_default();
                let msg = envelope("status", json!({
                    "queue_len": playback.queue.len(),
                    "now_playing": playback.now_playing,
                    "overlay_connected": state.tx.receiver_count() > 0,
                    "cookies": cookies_payload(data_dir),
                    "ytdlp_version": snapshot.ytdlp_version,
                    "disk_free_mb": 0,
                    "last_error": Value::Null
                }));
                if tx_ws.send(Message::Text(msg.to_string())).await.is_err() {
                    return SessionEnd::Retry("No se pudo enviar el estado.".into());
                }
            }
        }
    }
}

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    Message,
>;

fn envelope(kind: &str, payload: Value) -> Value {
    json!({
        "v": PROTOCOL_VERSION,
        "type": kind,
        "id": uuid_v4(),
        "ts": now_ms(),
        "payload": payload
    })
}

/// UUID v4 sin sumar una dependencia: el crate del pipeline ya trae `uuid`,
/// pero la app no, y esto solo se usa para correlacionar mensajes.
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    let seed = now_ms();
    for (i, byte) in b.iter_mut().enumerate() {
        // Mezcla barata: no hace falta calidad criptográfica para un id de
        // mensaje, solo que no se repita dentro de una sesión.
        *byte = ((seed >> ((i % 8) * 8)) as u8) ^ (i as u8).wrapping_mul(31);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = |r: &[u8]| r.iter().map(|x| format!("{x:02x}")).collect::<String>();
    format!("{}-{}-{}-{}-{}", h(&b[0..4]), h(&b[4..6]), h(&b[6..8]), h(&b[8..10]), h(&b[10..16]))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn cookies_payload(data_dir: &Path) -> Value {
    let st = core::cookies::read_status(data_dir);
    json!({
        "present": st.present,
        "state": match st.state {
            core::CookieState::Ok => "ok",
            core::CookieState::Expired => "expired",
            core::CookieState::Missing => "missing",
        },
        "last_ok_at": st.last_ok_at,
        "last_error_at": st.last_error_at
    })
}

async fn send_ready(state: &AppState, tx: &mut WsSink, data_dir: &Path) -> Result<(), ()> {
    let bins = core::binaries::resolve(data_dir);
    let version = core::ytdlp::version(&bins.ytdlp).await;
    let msg = envelope(
        "agent.ready",
        json!({
            "agent_version": env!("CARGO_PKG_VERSION"),
            "ytdlp_version": version,
            "ffmpeg_version": Value::Null,
            "cookies": cookies_payload(data_dir),
            "encoder": "h264_nvenc",
            // La cuenta real de Browser Sources conectados. Estaba fijo en
            // `true`, o sea que decía que OBS estaba enganchado incluso con OBS
            // cerrado, que es justo cuando importa saberlo.
            "overlay_connected": state.tx.receiver_count() > 0
        }),
    );
    tx.send(Message::Text(msg.to_string())).await.map_err(|_| ())
}

async fn apply_overlay_event(
    ev: OverlayEvent,
    media: &Path,
    playback: &mut Playback,
    tx: &mut WsSink,
) {
    match ev {
        OverlayEvent::Busy(busy) => playback.set_busy(busy),
        OverlayEvent::RequestStarted(item_id) => {
            playback.now_playing = Some(item_id.clone());
            let msg = envelope("playback.started", json!({ "item_id": item_id }));
            let _ = tx.send(Message::Text(msg.to_string())).await;
        }
        OverlayEvent::RequestEnded { item_id, seconds, reason } => {
            if playback.now_playing.as_deref() == Some(item_id.as_str()) {
                playback.now_playing = None;
            }
            // El motivo lo pone el overlay, no nosotros: el DO lo usa para dejar
            // el ítem en `played` o en `cleared`, y hasta acá se mandaba "ended"
            // siempre. Un video que ni arrancó figuraba como reproducido, y el
            // mod que lo aprobó no tenía forma de enterarse.
            let reason = if reason == "ended" { "ended" } else { "error" };
            logln!("[VideoRequests] fin {item_id}: {reason} ({seconds:.1}s)");
            // El archivo ya cumplió: la nube lo deja en `played` y no se vuelve a
            // encolar. Si no se borra acá, los mp4 se apilan toda la transmisión
            // —unos 5 MB cada uno— y recién se limpian en la próxima reconexión,
            // cuando el resync los declara huérfanos. Un stream largo sin
            // reconectar es un stream juntando basura en disco.
            core::pipeline::cleanup(media, &item_id);
            let msg = envelope(
                "playback.ended",
                json!({ "item_id": item_id, "reason": reason, "played_seconds": seconds }),
            );
            let _ = tx.send(Message::Text(msg.to_string())).await;
        }
    }
}

/// Una línea por mensaje que llega del hub.
///
/// Sin esto lo que pasa entre la nube y el agente es invisible: el streamer ve
/// "no se reprodujo" y no hay con qué distinguir un rechazo del mod, una
/// descarga que falló y un socket que se cayó.
fn log_hub(kind: &str, p: &Value) {
    match p["item_id"].as_str() {
        Some(id) if !id.is_empty() => logln!("[VideoRequests] ← {kind} {id}"),
        _ => logln!("[VideoRequests] ← {kind}"),
    }
}

// ── Mensajes del hub ─────────────────────────────────────────────────────────

/// Devuelve `Some(SessionEnd)` solo si hay que cortar la sesión.
async fn handle_hub_message(
    state: &AppState,
    v: &Value,
    bins: &core::Binaries,
    media: &Path,
    playback: &mut Playback,
    limits: &mut ChannelLimits,
    shared: &SharedVrStatus,
    tx: &mut WsSink,
) -> Option<SessionEnd> {
    let data_dir = state.data_dir.as_path();
    let kind = v["type"].as_str().unwrap_or("");
    let p = &v["payload"];
    log_hub(kind, p);

    match kind {
        "hello" => {
            if v["payload"]["protocol_version"].as_u64() != Some(PROTOCOL_VERSION) {
                return Some(SessionEnd::Terminal(
                    "El servidor usa otra versión del protocolo. Actualizá la app.".into(),
                ));
            }
            if let Some(gap) = p["settings"]["playback_gap_seconds"].as_u64() {
                playback.gap = Duration::from_secs(gap);
            }
            limits.apply(&p["settings"]);
        }

        "resync" => {
            // La nube manda la vista autoritativa: la cola local se descarta y
            // se adopta esta. Los archivos que ya no correspondan se borran.
            playback.queue.clear();
            let items = p["items"].as_array().cloned().unwrap_or_default();
            let mut keep: Vec<String> = Vec::new();
            for it in &items {
                let id = it["item_id"].as_str().unwrap_or("").to_string();
                if id.is_empty() {
                    continue;
                }
                keep.push(id.clone());
                let status = it["status"].as_str().unwrap_or("");
                let file = format!("{id}.mp4");
                if status == "ready" && media.join(&file).is_file() {
                    playback.queue.push_back(ReadyItem {
                        item_id: id,
                        file,
                        title: String::new(),
                        submitter: String::new(),
                    });
                }
            }
            remove_orphans(media, &keep);
            update(shared, |s| s.queue_len = playback.queue.len());
        }

        "metadata.request" => {
            let item_id = p["item_id"].as_str().unwrap_or("").to_string();
            let url = p["source_url"].as_str().unwrap_or("").to_string();
            let cfg = pipeline_config(data_dir, limits);
            let result = core::pipeline::fetch_metadata(bins, &url, &cfg).await;
            if let Err(e) = &result {
                logln!("[VideoRequests] metadata falló {item_id}: [{:?}] {}", e.code, e.message);
            }
            let payload = match result {
                Ok((_, meta)) => json!({
                    "item_id": item_id,
                    "ok": true,
                    "title": meta.title,
                    "duration_seconds": meta.duration_seconds,
                    "thumbnail_url": meta.thumbnail_url,
                    "uploader": meta.uploader
                }),
                Err(e) => {
                    note_cookie_failure(&e, shared);
                    json!({
                        "item_id": item_id,
                        "ok": false,
                        "error": { "code": e.code, "message": e.message, "retryable": e.retryable }
                    })
                }
            };
            let _ = tx.send(Message::Text(envelope("metadata.result", payload).to_string())).await;
        }

        "download.request" => {
            let item_id = p["item_id"].as_str().unwrap_or("").to_string();
            let url = p["source_url"].as_str().unwrap_or("").to_string();
            let cfg = pipeline_config(data_dir, limits);
            let result = core::pipeline::prepare(bins, &url, &item_id, media, &cfg).await;
            if let Err(e) = &result {
                logln!("[VideoRequests] descarga falló {item_id}: [{:?}] {}", e.code, e.message);
            }
            let payload = match result {
                Ok(prepared) => {
                    playback.queue.push_back(ReadyItem {
                        item_id: item_id.clone(),
                        file: format!("{item_id}.mp4"),
                        title: String::new(),
                        submitter: String::new(),
                    });
                    update(shared, |s| s.queue_len = playback.queue.len());
                    logln!(
                        "[VideoRequests] listo {item_id}: {:.1}s {}x{} ({})",
                        prepared.duration_seconds,
                        prepared.width,
                        prepared.height,
                        prepared.encoder_used.as_str()
                    );
                    json!({
                        "item_id": item_id,
                        "ok": true,
                        "duration_seconds": prepared.duration_seconds,
                        "width": prepared.width,
                        "height": prepared.height
                    })
                }
                Err(e) => {
                    note_cookie_failure(&e, shared);
                    json!({
                        "item_id": item_id,
                        "ok": false,
                        "error": { "code": e.code, "message": e.message, "retryable": e.retryable }
                    })
                }
            };
            let _ = tx.send(Message::Text(envelope("download.result", payload).to_string())).await;
        }

        "cancel" => {
            let item_id = p["item_id"].as_str().unwrap_or("");
            playback.queue.retain(|i| i.item_id != item_id);
            core::pipeline::cleanup(media, item_id);
            update(shared, |s| s.queue_len = playback.queue.len());
        }

        "clear" => {
            let keep_playing = p["keep_now_playing"].as_bool().unwrap_or(true);
            let playing = playback.now_playing.clone();
            playback.queue.clear();
            if let Ok(entries) = std::fs::read_dir(media) {
                for e in entries.flatten() {
                    let path = e.path();
                    let is_playing = playing
                        .as_deref()
                        .map(|id| path.file_stem().and_then(|s| s.to_str()) == Some(id))
                        .unwrap_or(false);
                    if !(keep_playing && is_playing) {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
            update(shared, |s| s.queue_len = 0);
        }

        "settings.update" => {
            if let Some(gap) = p["settings"]["playback_gap_seconds"].as_u64() {
                playback.gap = Duration::from_secs(gap);
            }
            limits.apply(&p["settings"]);
        }

        _ => {} // tipo desconocido: se ignora en silencio, como pide el contrato
    }
    None
}

/// La config con la que corre cada pedido.
///
/// Dos cosas que el DO ya mandaba y acá se descartaban: los límites del canal
/// (se recortaba todo a los 30 s de fábrica aunque /admin dijera otra cosa) y,
/// peor, las cookies de Instagram. Sin ellas el archivo que exporta el streamer
/// no lo usaba nadie: la UI decía "cargadas" y yt-dlp seguía yendo anónimo.
fn pipeline_config(data_dir: &Path, limits: &ChannelLimits) -> core::PipelineConfig {
    let mut cfg = core::PipelineConfig::default();
    cfg.max_duration_seconds = limits.max_duration_seconds;
    cfg.max_filesize_mb = limits.max_filesize_mb;
    // El lado corto manda; el largo sale de 16:9 sobre él (720→1280, 1080→1920).
    cfg.max_short_side = limits.max_resolution;
    cfg.max_long_side = limits.max_resolution * 16 / 9;
    // Solo si el archivo existe: un `--cookies` apuntando a la nada le hace
    // abortar la extracción entera a yt-dlp.
    if core::cookies::read_status(data_dir).present {
        cfg.cookies = Some(core::cookies::cookies_path(data_dir));
    }
    cfg
}

/// Un fallo por login degrada el estado de las cookies. El archivo no dice si
/// sirve; solo lo dice haberlo usado.
///
/// Y solo eso. El fallo de un pedido en particular NO va a `s.error`: la UI
/// lo pinta en la tarjeta de emparejamiento, en rojo, como si la conexión
/// estuviera rota. En stream el streamer vio "yt-dlp no pudo leer el video"
/// debajo de "conectado" por un link ajeno que ya figuraba como fallido en
/// /mod. Esos fallos quedan en el log y en la nube, que es donde el mod y el
/// viewer los ven.
fn note_cookie_failure(e: &core::ErrorDetail, shared: &SharedVrStatus) {
    if e.code == core::ErrorCode::CookiesExpired {
        update(shared, |s| {
            s.cookies_state = "expired".into();
            s.error = Some(e.message.clone());
        });
    }
}

/// Borra los mp4 que ya no están en la vista autoritativa de la nube.
fn remove_orphans(media: &Path, keep: &[String]) {
    let Ok(entries) = std::fs::read_dir(media) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let base = stem.trim_end_matches("-raw");
        if !keep.iter().any(|k| k == base) {
            let _ = std::fs::remove_file(&path);
        }
    }
}
