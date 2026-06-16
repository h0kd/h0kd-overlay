mod server;
mod twitch;

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
        "canvas": { "width": 1920, "height": 1080 }
    })
}

/// Per-user OS application-data directory for the app's config + videos.
/// Windows: %APPDATA%\Stream Overlay
/// macOS:   ~/Library/Application Support/Stream Overlay
/// Linux:   $XDG_DATA_HOME/Stream Overlay (or ~/.local/share/Stream Overlay)
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
                    println!("[Data] (dev) Found config.json at: {}", ancestor.display());
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
                println!("[Data] Portable: {}", parent.display());
                return parent.to_path_buf();
            }
        }
    }

    // Default for distributed apps: per-user OS app-data dir, seeded on first run.
    let dir = os_app_data_dir();
    ensure_data_dir(&dir);
    println!("[Data] Using app data dir: {}", dir.display());
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
    println!("[Trigger] playVideo → {} | clientes: {}", reward, clients);
    json!({ "ok": true, "clients": clients })
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
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no hay actualización disponible".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = Arc::new(find_data_dir());
    // Drop the initial receiver so the count reflects only real WS clients (overlay connections).
    // tx.send() will return SendError when no subscribers exist; trigger_reward handles that via unwrap_or(0).
    let (tx, _) = broadcast::channel::<String>(64);

    let twitch_shared = Arc::new(Mutex::new(TwitchStatus::default()));
    let (twitch_cmd, twitch_rx) = mpsc::channel::<TwitchCmd>(8);

    let state = AppState {
        tx: tx.clone(),
        data_dir: data_dir.clone(),
        twitch: twitch_shared,
        twitch_cmd,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state.clone())
        .setup(move |_| {
            let server_state = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = server::start(server_state).await {
                    eprintln!("[Server] failed to start: {}", e);
                }
            });
            // Twitch EventSub worker: connects to Twitch and broadcasts redemptions.
            let twitch_state = state.clone();
            tauri::async_runtime::spawn(twitch::worker_loop(twitch_state, twitch_rx));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            list_videos,
            trigger_reward,
            open_data_dir,
            open_url,
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
