mod applog;
mod server;
mod twitch;
mod video_requests;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};
use twitch::{TwitchCmd, TwitchStatus};

#[derive(Clone)]
pub struct AppState {
    pub tx: broadcast::Sender<String>,
    pub data_dir: Arc<PathBuf>,
    pub twitch: twitch::SharedStatus,
    pub twitch_cmd: mpsc::Sender<TwitchCmd>,
    /// Whether THIS instance's HTTP/WS server bound the port. The only reliable
    /// signal: an HTTP ping to the port can succeed against a *different* process
    /// (e.g. a stale instance) and falsely look healthy.
    pub server_health: Arc<Mutex<ServerHealth>>,
    /// Canal de comandos del módulo Video Requests. `None` cuando el flag
    /// está apagado, que es lo que hace verificable la regla "apagado = igual
    /// que sin el módulo": sin este Sender no hay a quién hablarle.
    pub vr_cmd: Option<mpsc::Sender<video_requests::VrCmd>>,
    /// Estado del módulo, para que la UI lo lea por polling.
    pub video_requests: Option<video_requests::SharedVrStatus>,
    /// Video Requests feature flag, read ONCE at startup from
    /// `videoRequests.enabled`. False means the module is never
    /// constructed: no sidecars, no cloud WebSocket, no extra routes, no
    /// UI beyond the toggle. Reading it once (instead of watching the
    /// config) is deliberate — it makes "off = identical to no module"
    /// verifiable by reading `run()`.
    pub video_requests_enabled: bool,
    /// Espejo mudo de lo que reproduce el overlay, para la ventana de
    /// preview (VRChat). Existe siempre; sin el flag nunca recibe nada.
    pub preview: server::PreviewHub,
}

/// Reported to the UI so it can warn when the local server didn't start.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ServerHealth {
    Starting,
    Ok,
    Error { message: String },
}

/// Broadcast a `playVideo` event to every connected overlay.
/// Single source of truth shared by the admin "Probar" button and the Twitch
/// EventSub listener. Returns the number of overlays reached.
pub fn broadcast_play_video(tx: &broadcast::Sender<String>, reward: &str, user: &str) -> usize {
    let msg = json!({
        "event": { "source": "General", "type": "Custom" },
        "data": { "action": "playVideo", "reward": reward, "user": user }
    })
    .to_string();
    tx.send(msg).unwrap_or(0)
}

fn default_config() -> Value {
    json!({
        "rewards": {},
        "safeZones": { "exclude": [] },
        "canvas": { "width": 1920, "height": 1080 },
        // El módulo viene APAGADO: cada canal lo activa con el switch si
        // quiere (decisión de h0kd). Lo que sí viene puesto es el Worker por
        // defecto, así activar + emparejar no requiere tocar AppData. Esto se
        // escribe solo la primera vez; una config existente no se toca.
        "videoRequests": { "enabled": false, "workerOrigin": video_requests::DEFAULT_WORKER_ORIGIN }
    })
}

/// Read the Video Requests flag. A missing key, an unreadable file or
/// malformed JSON all mean *disabled*: the module must never switch itself
/// on by accident, and this runs before anything else is initialized.
fn read_video_requests_flag(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["videoRequests"]["enabled"].as_bool())
        .unwrap_or(false)
}

/// Per-user OS application-data directory for the app's config + videos.
/// Windows: %APPDATA%\Stream Overlay
/// macOS:   ~/Library/Application Support/Stream Overlay
/// Linux:   $XDG_DATA_HOME/Stream Overlay
fn os_app_data_dir() -> PathBuf {
    let base: PathBuf = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."))
    };
    base.join("Stream Overlay")
}

/// Create the data dir + videos/ folder, and seed a default config.json the
/// first time the app runs, so a freshly downloaded app works out of the box.
fn ensure_data_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir.join("videos"));
    let cfg = dir.join("config.json");
    if !cfg.exists() {
        if let Ok(pretty) = serde_json::to_string_pretty(&default_config()) {
            let _ = std::fs::write(&cfg, pretty);
        }
    }
}

/// One-time hand-off from the experimental build ("Stream Overlay
/// Experimental", exp-v0.2.10 to exp-v0.2.13), which shipped Video Requests
/// before it merged into the stable app. It kept its own data dir, so the
/// pairing with the cloud, the downloaded yt-dlp/ffmpeg, the Instagram
/// cookies and the preview-window preference only exist there. This copies
/// the whole `video-requests/` folder (minus the `media/` cache) and the
/// `videoRequests` block of its config.json — nothing else: rewards, videos
/// and Twitch tokens already live in the stable dir.
///
/// Runs only while the stable dir has no `video-requests/` folder yet, so it
/// can never overwrite a setup that is already working. The experimental
/// folder is left untouched; the user uninstalls that app whenever.
fn migrate_from_experimental(dir: &Path) {
    let old = match dir.parent() {
        Some(p) => p.join("Stream Overlay Experimental"),
        None => return,
    };
    let old_vr = old.join("video-requests");
    let new_vr = dir.join("video-requests");
    if !old_vr.is_dir() || new_vr.exists() {
        return;
    }
    logln!("[Data] Migrating Video Requests from {}", old.display());
    if let Err(e) = copy_dir_skipping(&old_vr, &new_vr, "media") {
        logln!("[Data] Migration of video-requests/ failed: {}", e);
        return;
    }
    let old_cfg = std::fs::read_to_string(old.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let block = match old_cfg.and_then(|v| v.get("videoRequests").cloned()) {
        Some(b) if b.is_object() => b,
        _ => return,
    };
    let cfg_path = dir.join("config.json");
    let mut cfg = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(default_config);
    cfg["videoRequests"] = block;
    match serde_json::to_string_pretty(&cfg) {
        Ok(pretty) => {
            if let Err(e) = std::fs::write(&cfg_path, pretty) {
                logln!("[Data] Could not write migrated config.json: {}", e);
            } else {
                logln!("[Data] Migrated videoRequests config (enabled={})",
                    cfg["videoRequests"]["enabled"].as_bool().unwrap_or(false));
            }
        }
        Err(e) => logln!("[Data] Could not serialize migrated config.json: {}", e),
    }
}

/// Recursive copy of `from` into `to`, skipping the top-level entry named
/// `skip` (the download cache, which is big and rebuilt on demand).
fn copy_dir_skipping(from: &Path, to: &Path, skip: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == skip {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_dir_skipping(&src, &dst, "")?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn find_data_dir() -> PathBuf {
    // Dev builds: use the repo's config.json (walk ancestors of cwd and the exe)
    // so `cargo run` / `cargo tauri dev` iterate against the checked-in config.
    #[cfg(debug_assertions)]
    {
        let mut roots: Vec<PathBuf> = vec![];
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                roots.push(parent.to_path_buf());
            }
        }
        for root in &roots {
            for ancestor in root.ancestors() {
                if ancestor.join("config.json").exists() {
                    logln!("[Data] (dev) Found config.json at: {}", ancestor.display());
                    return ancestor.to_path_buf();
                }
            }
        }
    }

    // Portable mode: a config.json sitting next to the executable takes priority,
    // letting advanced users keep everything in one folder.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("config.json").exists() {
                logln!("[Data] Portable: {}", parent.display());
                return parent.to_path_buf();
            }
        }
    }

    // Default for distributed apps: per-user OS app-data dir, seeded on first run.
    let dir = os_app_data_dir();
    ensure_data_dir(&dir);
    migrate_from_experimental(&dir);
    logln!("[Data] Using app data dir: {}", dir.display());
    dir
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> Value {
    let path = state.data_dir.join("config.json");
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| default_config()),
        Err(_) => default_config(),
    }
}

#[tauri::command]
fn save_config(state: tauri::State<AppState>, cfg: Value) -> Result<(), String> {
    let pretty = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(state.data_dir.join("config.json"), pretty).map_err(|e| e.to_string())?;
    // Tell connected overlays to reload their config so size/volume/etc changes
    // apply immediately, without refreshing the OBS Browser Source.
    let _ = state.tx.send(reload_config_msg());
    Ok(())
}

/// Message that asks every connected overlay to re-fetch config.json.
fn reload_config_msg() -> String {
    json!({
        "event": { "source": "System", "type": "Custom" },
        "data": { "action": "reloadConfig" }
    })
    .to_string()
}

#[tauri::command]
fn list_videos(state: tauri::State<AppState>) -> Vec<String> {
    let dir = state.data_dir.join("videos");
    let mut files: Vec<String> = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".mp4") || lower.ends_with(".webm") || lower.ends_with(".mov") {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

#[tauri::command]
fn trigger_reward(state: tauri::State<AppState>, reward: String, user: Option<String>) -> Value {
    let user = user.unwrap_or_default();
    let clients = broadcast_play_video(&state.tx, &reward, &user);
    logln!("[Trigger] playVideo → {} | clientes: {}", reward, clients);
    json!({ "ok": true, "clients": clients })
}

#[tauri::command]
fn server_status(state: tauri::State<AppState>) -> ServerHealth {
    state.server_health.lock().map(|h| h.clone()).unwrap_or(ServerHealth::Starting)
}

/// How many overlays (OBS Browser Sources) are currently connected via WS.
/// Every `handle_ws` subscribes to the broadcast channel, so the receiver count
/// equals the number of live overlay connections. The control panel polls this
/// to show a persistent "overlay connected" indicator so the streamer can
/// confirm OBS is hooked up *before* going live, instead of finding out on a
/// real redemption.
#[tauri::command]
fn overlay_count(state: tauri::State<AppState>) -> usize {
    state.tx.receiver_count()
}

// ── Twitch (direct EventSub integration) ─────────────────────────────────────

#[tauri::command]
fn twitch_status(state: tauri::State<AppState>) -> TwitchStatus {
    state.twitch.lock().map(|s| s.clone()).unwrap_or_default()
}

#[tauri::command]
async fn twitch_set_client_id(
    state: tauri::State<'_, AppState>,
    client_id: String,
) -> Result<(), String> {
    let cmd = state.twitch_cmd.clone();
    cmd.send(TwitchCmd::SetClientId(client_id.trim().to_string()))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn twitch_connect(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let cmd = state.twitch_cmd.clone();
    cmd.send(TwitchCmd::Connect).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn twitch_disconnect(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let cmd = state.twitch_cmd.clone();
    cmd.send(TwitchCmd::Disconnect)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn twitch_rewards(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<twitch::RewardInfo>, String> {
    let data_dir = state.data_dir.clone();
    twitch::fetch_channel_rewards(data_dir.as_path()).await
}

#[tauri::command]
fn open_data_dir(state: tauri::State<AppState>) -> Result<(), String> {
    // `open` opens the folder with the OS file manager on every platform.
    open::that(state.data_dir.as_path()).map_err(|e| e.to_string())
}

/// Open an external URL in the system's default browser. Webview `<a target="_blank">`
/// links don't reach the OS browser, so the frontend routes clicks through this.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    // Only allow web URLs; never hand arbitrary strings to the opener.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) URLs are allowed".into());
    }
    // `open` routes to the default browser correctly on each OS (on Windows,
    // `explorer <url>` misbehaves and opens File Explorer instead).
    open::that(&url).map_err(|e| e.to_string())
}

/// Base URL of THIS build's local server. The control panel builds every
/// link and fetch from it instead of hardcoding a port.
#[tauri::command]
fn server_url() -> String {
    format!("http://127.0.0.1:{}", server::SERVER_PORT)
}

/// Whether the Video Requests module is live *in this process*. The UI
/// compares it against the saved flag to tell the user a restart is pending.
#[tauri::command]
fn video_requests_active(state: tauri::State<AppState>) -> bool {
    state.video_requests_enabled
}

// ── Video Requests ─────────────────────────────────────────────────────────

/// Estado del módulo para la UI. Con el flag apagado devuelve el estado por
/// defecto en vez de fallar: la sección existe siempre, el módulo no.
#[tauri::command]
fn vr_status(state: tauri::State<AppState>) -> video_requests::VrStatus {
    match &state.video_requests {
        Some(s) => s.lock().map(|v| v.clone()).unwrap_or_default(),
        None => video_requests::VrStatus::default(),
    }
}

async fn vr_send(state: &AppState, cmd: video_requests::VrCmd) -> Result<(), String> {
    let tx = state
        .vr_cmd
        .clone()
        .ok_or_else(|| "El módulo está apagado. Activalo y reiniciá la app.".to_string())?;
    tx.send(cmd).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn vr_pair(state: tauri::State<'_, AppState>, code: String) -> Result<(), String> {
    vr_send(&state, video_requests::VrCmd::Pair(code)).await
}

#[tauri::command]
async fn vr_unpair(state: tauri::State<'_, AppState>) -> Result<(), String> {
    vr_send(&state, video_requests::VrCmd::Unpair).await
}

#[tauri::command]
async fn vr_install_binaries(state: tauri::State<'_, AppState>) -> Result<(), String> {
    vr_send(&state, video_requests::VrCmd::InstallBinaries).await
}

#[tauri::command]
async fn vr_update_ytdlp(state: tauri::State<'_, AppState>) -> Result<(), String> {
    vr_send(&state, video_requests::VrCmd::UpdateYtdlp).await
}

#[tauri::command]
async fn vr_reconnect(state: tauri::State<'_, AppState>) -> Result<(), String> {
    vr_send(&state, video_requests::VrCmd::Reconnect).await
}

/// Abre la carpeta donde va el archivo de cookies de Instagram, creándola si
/// hace falta. Explicarle a alguien cómo llegar a %APPDATA% a mano es fricción
/// gratis.
#[tauri::command]
fn open_logs_dir(state: tauri::State<AppState>) -> Result<(), String> {
    let dir = applog::dir(&state.data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    open::that(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
fn vr_open_cookies_dir(state: tauri::State<AppState>) -> Result<(), String> {
    let dir = state.data_dir.join("video-requests");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    open::that(&dir).map_err(|e| e.to_string())
}

// ── Ventana de preview (Video Requests, para VRChat) ─────────────────────────
// Una segunda ventana de la app que muestra, sin sonido, el pedido que el
// overlay de OBS está reproduciendo. XSOverlay (o cualquier overlay de
// escritorio) la captura y el streamer lo ve en grande dentro de VRChat.
// Es opcional: la preferencia vive en su propio archivo, aparte de
// config.json, para no meterse en el guardar/deshacer del panel.

const PREVIEW_LABEL: &str = "vr-preview";

fn preview_pref_path(data_dir: &Path) -> PathBuf {
    data_dir.join("video-requests").join("preview-window.json")
}

fn read_preview_pref(data_dir: &Path) -> bool {
    std::fs::read_to_string(preview_pref_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["open"].as_bool())
        .unwrap_or(false)
}

fn write_preview_pref(data_dir: &Path, open: bool) {
    let path = preview_pref_path(data_dir);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, json!({ "open": open }).to_string());
}

fn open_preview_window(app: &tauri::AppHandle, data_dir: &Path) -> Result<(), String> {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let window = tauri::WebviewWindowBuilder::new(
        app,
        PREVIEW_LABEL,
        tauri::WebviewUrl::App("preview.html".into()),
    )
    .title("Video Requests — Preview")
    .inner_size(960.0, 540.0)
    .min_inner_size(320.0, 180.0)
    .build()
    .map_err(|e| e.to_string())?;

    // Cerrarla con la X la desactiva: si no, reaparecería en el próximo
    // inicio y no habría forma de sacársela de encima desde la ventana.
    let dir = data_dir.to_path_buf();
    let handle = app.clone();
    window.on_window_event(move |ev| {
        if let tauri::WindowEvent::CloseRequested { .. } = ev {
            use tauri::Emitter;
            write_preview_pref(&dir, false);
            let _ = handle.emit_to("control", "vr-preview", false);
        }
    });
    Ok(())
}

/// Abre o cierra la ventana de preview y recuerda la elección.
///
/// Es `async` a propósito: en Windows, crear una ventana desde un comando
/// síncrono corre en el hilo principal y se traba esperando al event loop
/// (la ventana aparece vacía, congelada y sin poder cerrarse). Con el
/// comando en el runtime async, la creación se despacha bien.
#[tauri::command]
async fn vr_preview_window(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    open: bool,
) -> Result<bool, String> {
    use tauri::Manager;
    if !state.video_requests_enabled {
        return Err("Video Requests no está activo en este proceso.".into());
    }
    write_preview_pref(&state.data_dir, open);
    if open {
        open_preview_window(&app, &state.data_dir)?;
    } else if let Some(w) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = w.close();
    }
    Ok(open)
}

/// ¿Está abierta ahora? Es lo que el panel muestra en el switch.
#[tauri::command]
fn vr_preview_open(app: tauri::AppHandle) -> bool {
    use tauri::Manager;
    app.get_webview_window(PREVIEW_LABEL).is_some()
}

// ── Auto-update (tauri-plugin-updater) ───────────────────────────────────────

#[derive(serde::Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

/// Check the release endpoint for a newer signed version. Returns Some(info)
/// when an update is available, None when up to date. Errors (offline, no
/// endpoint in dev) are surfaced so the UI can stay silent.
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(update) => Ok(Some(UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone(),
        })),
        None => Ok(None),
    }
}

/// Download + install the available update, then relaunch into the new version.
/// Emits `update://progress` events (0–100) so the UI can show a percentage.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no hay actualización disponible".to_string())?;

    let mut downloaded: u64 = 0;
    let mut last_pct: i64 = -1;
    let on_chunk = {
        let app = app.clone();
        move |chunk: usize, total: Option<u64>| {
            downloaded += chunk as u64;
            // Sin Content-Length no hay porcentaje; el front cae a "Descargando…".
            if let Some(total) = total.filter(|t| *t > 0) {
                let pct = ((downloaded * 100) / total).min(100) as i64;
                // Emit solo cuando cambia el entero, para no inundar el front.
                if pct != last_pct {
                    last_pct = pct;
                    let _ = app.emit("update://progress", pct);
                }
            }
        }
    };
    let on_finish = {
        let app = app.clone();
        move || {
            // 100% = descarga lista; ahora corre el instalador.
            let _ = app.emit("update://progress", 100i64);
        }
    };

    update
        .download_and_install(on_chunk, on_finish)
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = Arc::new(find_data_dir());
    // Antes que cualquier otra cosa: si algo falla en el arranque, queremos que
    // ese fallo tambien quede escrito.
    applog::init(&data_dir);
    logln!("[App] {} arranca. Log en {}", env!("CARGO_PKG_VERSION"), applog::dir(&data_dir).display());
    let video_requests_enabled = read_video_requests_flag(&data_dir);
    if video_requests_enabled {
        logln!("[VideoRequests] Flag activo.");
    }
    // Drop the initial receiver so the count reflects only real WS clients (overlay connections).
    // tx.send() will return SendError when no subscribers exist; trigger_reward handles that via unwrap_or(0).
    let (tx, _) = broadcast::channel::<String>(64);

    let twitch_shared = Arc::new(Mutex::new(TwitchStatus::default()));
    let (twitch_cmd, twitch_rx) = mpsc::channel::<TwitchCmd>(8);

    // Con el flag apagado esto queda en `None` y el worker no se lanza: no hay
    // WebSocket a la nube, no hay procesos hijo y no se registran rutas nuevas.
    let (vr_cmd, vr_rx) = if video_requests_enabled {
        let (tx, rx) = mpsc::channel::<video_requests::VrCmd>(32);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let vr_shared = video_requests_enabled
        .then(|| Arc::new(Mutex::new(video_requests::VrStatus::default())));

    let state = AppState {
        tx: tx.clone(),
        data_dir: data_dir.clone(),
        twitch: twitch_shared,
        twitch_cmd,
        server_health: Arc::new(Mutex::new(ServerHealth::Starting)),
        vr_cmd,
        video_requests: vr_shared,
        video_requests_enabled,
        preview: server::PreviewHub::new(),
    };

    tauri::Builder::default()
        // Single-instance must be the FIRST plugin. If the app is already
        // running, the new process exits and we just focus the existing window
        // instead of launching a duplicate that can't bind the port.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("control") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state.clone())
        .setup(move |app| {
            // Cerrar el panel cierra la app entera. La preview se DESTRUYE
            // (sin CloseRequested): así cerrar la app no desmarca la opción y
            // la ventana vuelve a abrirse en el próximo inicio.
            {
                use tauri::Manager;
                let handle = app.handle().clone();
                if let Some(control) = app.get_webview_window("control") {
                    control.on_window_event(move |ev| {
                        if let tauri::WindowEvent::CloseRequested { .. } = ev {
                            if let Some(p) = handle.get_webview_window(PREVIEW_LABEL) {
                                let _ = p.destroy();
                            }
                        }
                    });
                }
                if state.video_requests_enabled && read_preview_pref(&state.data_dir) {
                    if let Err(e) = open_preview_window(app.handle(), &state.data_dir) {
                        logln!("[Preview] No se pudo abrir la ventana: {}", e);
                    }
                }
            }
            let server_state = state.clone();
            let health = server_state.server_health.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = server::start(server_state).await {
                    let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
                        format!(
                            "El puerto {} ya está en uso por otra aplicación. \
                             Cerrá la otra instancia (o el programa que lo ocupa) y reabrí la app.",
                            server::SERVER_PORT
                        )
                    } else {
                        format!("No se pudo iniciar el servidor local: {}", e)
                    };
                    logln!("[Server] {}", msg);
                    if let Ok(mut h) = health.lock() {
                        *h = ServerHealth::Error { message: msg };
                    }
                }
            });
            // Twitch EventSub worker: connects to Twitch and broadcasts redemptions.
            let twitch_state = state.clone();
            tauri::async_runtime::spawn(twitch::worker_loop(twitch_state, twitch_rx));

            // Solo existe si el flag estaba encendido al arrancar.
            if let Some(rx) = vr_rx {
                let vr_state = state.clone();
                tauri::async_runtime::spawn(video_requests::worker_loop(vr_state, rx));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            list_videos,
            trigger_reward,
            server_status,
            overlay_count,
            open_data_dir,
            open_url,
            server_url,
            video_requests_active,
            vr_status,
            vr_pair,
            vr_unpair,
            vr_install_binaries,
            vr_update_ytdlp,
            vr_reconnect,
            vr_open_cookies_dir,
            vr_preview_window,
            vr_preview_open,
            open_logs_dir,
            twitch_status,
            twitch_set_client_id,
            twitch_connect,
            twitch_disconnect,
            twitch_rewards,
            check_update,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    fn fresh_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("so-migrate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn seed_experimental(root: &Path, enabled: bool) {
        let old = root.join("Stream Overlay Experimental");
        std::fs::create_dir_all(old.join("video-requests/bin")).unwrap();
        std::fs::create_dir_all(old.join("video-requests/media")).unwrap();
        std::fs::write(old.join("video-requests/pairing.json"), r#"{"token":"abc"}"#).unwrap();
        std::fs::write(old.join("video-requests/bin/yt-dlp.exe"), b"bin").unwrap();
        std::fs::write(old.join("video-requests/media/cache.mp4"), b"big").unwrap();
        let cfg = json!({
            "rewards": [{ "name": "viejo" }],
            "videoRequests": { "enabled": enabled, "workerOrigin": "https://videos.h0kd.dev", "layout": { "size": 62 } }
        });
        std::fs::write(old.join("config.json"), cfg.to_string()).unwrap();
    }

    #[test]
    fn copia_pairing_binarios_y_config_pero_no_el_cache() {
        let root = fresh_root("basic");
        seed_experimental(&root, true);
        let dir = root.join("Stream Overlay");
        ensure_data_dir(&dir);
        let stable = json!({ "rewards": [{ "name": "r1" }, { "name": "r2" }], "canvas": { "width": 1920 } });
        std::fs::write(dir.join("config.json"), stable.to_string()).unwrap();

        migrate_from_experimental(&dir);

        assert!(dir.join("video-requests/pairing.json").is_file());
        assert!(dir.join("video-requests/bin/yt-dlp.exe").is_file());
        assert!(!dir.join("video-requests/media").exists(), "el cache no se copia");
        let cfg: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).unwrap()).unwrap();
        assert_eq!(cfg["rewards"].as_array().unwrap().len(), 2, "las rewards de la estable quedan");
        assert_eq!(cfg["canvas"]["width"], 1920);
        assert_eq!(cfg["videoRequests"]["enabled"], true);
        assert_eq!(cfg["videoRequests"]["layout"]["size"], 62);
        assert!(root.join("Stream Overlay Experimental/video-requests/pairing.json").is_file(), "la vieja no se toca");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_pisa_una_instalacion_que_ya_tiene_video_requests() {
        let root = fresh_root("existing");
        seed_experimental(&root, true);
        let dir = root.join("Stream Overlay");
        ensure_data_dir(&dir);
        std::fs::create_dir_all(dir.join("video-requests")).unwrap();
        std::fs::write(dir.join("video-requests/pairing.json"), r#"{"token":"mio"}"#).unwrap();

        migrate_from_experimental(&dir);

        assert_eq!(std::fs::read_to_string(dir.join("video-requests/pairing.json")).unwrap(), r#"{"token":"mio"}"#);
        let cfg: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).unwrap()).unwrap();
        assert_eq!(cfg["videoRequests"]["enabled"], false, "la config sembrada no se toca");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sin_carpeta_experimental_no_hace_nada() {
        let root = fresh_root("none");
        let dir = root.join("Stream Overlay");
        ensure_data_dir(&dir);
        migrate_from_experimental(&dir);
        assert!(!dir.join("video-requests").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
