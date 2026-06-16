//! Direct Twitch integration — the app connects to Twitch on its own.
//!
//! Flow:
//!   1. OAuth Device Code Flow → obtain a user access token (`channel:read:redemptions`).
//!   2. EventSub WebSocket → subscribe to channel point redemptions.
//!   3. On each redemption → broadcast a `playVideo` message to every connected
//!      overlay (via `broadcast_play_video`, same path as the "Probar" button).

use crate::AppState;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Shared "Stream Overlay by h0kd" Twitch App (Public client). The Client ID is
/// NOT a secret in the Device Code Flow, so it's safe to ship embedded — this is
/// what lets any streamer connect without registering their own Twitch App.
/// Advanced users can still override it (stored per-machine in twitch.json).
const DEFAULT_CLIENT_ID: &str = "zwq84d8wq9lrkvgg3pb1pwe26s8794";

/// The Client ID to actually use: the user's own if set, otherwise the shared one.
fn effective_client_id(saved: &str) -> String {
    let s = saved.trim();
    if s.is_empty() { DEFAULT_CLIENT_ID.to_string() } else { s.to_string() }
}

const SCOPES: &str = "channel:read:redemptions";
const DEVICE_URL: &str = "https://id.twitch.tv/oauth2/device";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const HELIX_USERS: &str = "https://api.twitch.tv/helix/users";
const HELIX_REWARDS: &str = "https://api.twitch.tv/helix/channel_points/custom_rewards";
const HELIX_SUBS: &str = "https://api.twitch.tv/helix/eventsub/subscriptions";
const EVENTSUB_WS: &str = "wss://eventsub.wss.twitch.tv/ws";
const REDEMPTION_TYPE: &str = "channel.channel_points_custom_reward_redemption.add";

// ── Shared status (read by the UI via the `twitch_status` command) ───────────

#[derive(Clone, Serialize)]
pub struct TwitchStatus {
    /// "disconnected" | "pairing" | "connecting" | "connected" | "error"
    pub state: String,
    #[serde(rename = "clientId")]
    pub client_id: Option<String>,
    /// True when using the shared embedded Client ID (no user override set).
    #[serde(rename = "usingDefault")]
    pub using_default: bool,
    #[serde(rename = "userCode")]
    pub user_code: Option<String>,
    #[serde(rename = "verificationUri")]
    pub verification_uri: Option<String>,
    pub login: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub error: Option<String>,
}

impl Default for TwitchStatus {
    fn default() -> Self {
        TwitchStatus {
            state: "disconnected".into(),
            client_id: None,
            using_default: true,
            user_code: None,
            verification_uri: None,
            login: None,
            display_name: None,
            avatar: None,
            error: None,
        }
    }
}

pub type SharedStatus = Arc<Mutex<TwitchStatus>>;

fn update_status(shared: &SharedStatus, f: impl FnOnce(&mut TwitchStatus)) {
    if let Ok(mut s) = shared.lock() {
        f(&mut s);
    }
}

// ── Commands from the UI to the background worker ────────────────────────────

pub enum TwitchCmd {
    SetClientId(String),
    Connect,
    Disconnect,
}

// ── Token persistence (data_dir/twitch.json — gitignored) ────────────────────

#[derive(Serialize, Deserialize, Default, Clone)]
struct Tokens {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
}

fn tokens_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("twitch.json")
}

fn load_tokens(data_dir: &Path) -> Tokens {
    match std::fs::read_to_string(tokens_path(data_dir)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Tokens::default(),
    }
}

fn save_tokens(data_dir: &Path, tokens: &Tokens) {
    if let Ok(s) = serde_json::to_string_pretty(tokens) {
        let _ = std::fs::write(tokens_path(data_dir), s);
    }
}

// ── Twitch API DTOs ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
}

// ── Errors that drive the reconnect loop ─────────────────────────────────────

enum SessionError {
    /// Token rejected (401) — try a refresh.
    Auth,
    /// Transient: connection dropped / keepalive timeout — just reconnect.
    Reconnect,
    /// Unrecoverable for this attempt — surface to UI, back off, retry.
    Fatal(String),
}

// ── Worker entry point (spawned once at startup) ─────────────────────────────

pub async fn worker_loop(state: AppState, mut rx: mpsc::Receiver<TwitchCmd>) {
    let shared = state.twitch.clone();
    let data_dir = state.data_dir.clone();
    let mut handle: Option<tokio::task::JoinHandle<()>> = None;

    // Report the effective client id (user's own, or the shared default) so the
    // UI skips the "register a Twitch App" step and goes straight to Connect.
    let saved = load_tokens(&data_dir);
    update_status(&shared, |s| {
        s.client_id = Some(effective_client_id(&saved.client_id));
        s.using_default = saved.client_id.trim().is_empty();
    });
    if !saved.access_token.is_empty() {
        handle = Some(tokio::spawn(session(state.clone())));
    }

    while let Some(cmd) = rx.recv().await {
        match cmd {
            TwitchCmd::SetClientId(id) => {
                let mut tok = load_tokens(&data_dir);
                tok.client_id = id.clone();
                save_tokens(&data_dir, &tok);
                // Empty input means "reset to the shared default".
                update_status(&shared, |s| {
                    s.client_id = Some(effective_client_id(&id));
                    s.using_default = id.trim().is_empty();
                });
            }
            TwitchCmd::Connect => {
                if let Some(h) = handle.take() {
                    h.abort();
                }
                update_status(&shared, |s| {
                    s.state = "connecting".into();
                    s.error = None;
                });
                handle = Some(tokio::spawn(session(state.clone())));
            }
            TwitchCmd::Disconnect => {
                if let Some(h) = handle.take() {
                    h.abort();
                }
                let mut tok = load_tokens(&data_dir);
                tok.access_token.clear();
                tok.refresh_token.clear();
                save_tokens(&data_dir, &tok);
                update_status(&shared, |s| {
                    s.state = "disconnected".into();
                    s.user_code = None;
                    s.verification_uri = None;
                    s.login = None;
                    s.display_name = None;
                    s.avatar = None;
                    s.error = None;
                });
                println!("[Twitch] Desconectado.");
            }
        }
    }
}

// ── A full session: auth (if needed) + EventSub with reconnect ───────────────

async fn session(state: AppState) {
    let shared = state.twitch.clone();
    let data_dir = state.data_dir.clone();
    let client = reqwest::Client::new();

    let mut tokens = load_tokens(&data_dir);
    // Fall back to the shared Client ID when the user hasn't set their own.
    tokens.client_id = effective_client_id(&tokens.client_id);

    // Run the device flow when there is no usable access token yet.
    if tokens.access_token.is_empty() {
        match device_flow(&client, &tokens.client_id, &shared).await {
            Ok(t) => {
                tokens.access_token = t.access_token;
                tokens.refresh_token = t.refresh_token;
                save_tokens(&data_dir, &tokens);
            }
            Err(e) => {
                update_status(&shared, |s| {
                    s.state = "error".into();
                    s.user_code = None;
                    s.verification_uri = None;
                    s.error = Some(e);
                });
                return;
            }
        }
    }

    update_status(&shared, |s| {
        s.state = "connecting".into();
        s.user_code = None;
        s.verification_uri = None;
        s.error = None;
    });

    // EventSub connection loop with token refresh + backoff.
    let mut auth_retries = 0u32;
    loop {
        match run_eventsub(&client, &state, &tokens).await {
            Err(SessionError::Auth) => {
                if auth_retries >= 1 {
                    update_status(&shared, |s| {
                        s.state = "error".into();
                        s.login = None;
                        s.display_name = None;
                        s.avatar = None;
                        s.error = Some("Sesión expirada. Reconectá con Twitch.".into());
                    });
                    // Drop the dead token so the next Connect runs the device flow.
                    tokens.access_token.clear();
                    tokens.refresh_token.clear();
                    save_tokens(&data_dir, &tokens);
                    return;
                }
                auth_retries += 1;
                match refresh(&client, &tokens.client_id, &tokens.refresh_token).await {
                    Ok(t) => {
                        tokens.access_token = t.access_token;
                        tokens.refresh_token = t.refresh_token;
                        save_tokens(&data_dir, &tokens);
                        println!("[Twitch] Token renovado.");
                    }
                    Err(e) => {
                        update_status(&shared, |s| {
                            s.state = "error".into();
                            s.login = None;
                            s.display_name = None;
                            s.avatar = None;
                            s.error = Some(format!("No se pudo renovar el token: {e}"));
                        });
                        tokens.access_token.clear();
                        tokens.refresh_token.clear();
                        save_tokens(&data_dir, &tokens);
                        return;
                    }
                }
            }
            Err(SessionError::Reconnect) => {
                auth_retries = 0;
                update_status(&shared, |s| {
                    if s.state == "connected" {
                        s.state = "connecting".into();
                    }
                });
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(SessionError::Fatal(e)) => {
                update_status(&shared, |s| {
                    s.state = "error".into();
                    s.error = Some(e);
                });
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Ok(()) => {
                // run_eventsub only returns Ok on graceful close; reconnect.
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

// ── OAuth Device Code Flow ───────────────────────────────────────────────────

async fn device_flow(
    client: &reqwest::Client,
    client_id: &str,
    shared: &SharedStatus,
) -> Result<TokenResponse, String> {
    let dev: DeviceResponse = client
        .post(DEVICE_URL)
        .form(&[("client_id", client_id), ("scopes", SCOPES)])
        .send()
        .await
        .map_err(|e| format!("Error al pedir el device code: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Twitch rechazó el device code (¿Client ID válido?): {e}"))?
        .json()
        .await
        .map_err(|e| format!("Respuesta de device inválida: {e}"))?;

    println!(
        "[Twitch] Emparejá: andá a {} y poné el código {}",
        dev.verification_uri, dev.user_code
    );
    update_status(shared, |s| {
        s.state = "pairing".into();
        s.user_code = Some(dev.user_code.clone());
        s.verification_uri = Some(dev.verification_uri.clone());
        s.error = None;
    });

    let interval = dev.interval.unwrap_or(5).max(1);
    // Poll the token endpoint until the user authorizes (or it times out).
    for _ in 0..180 {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let resp = client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", client_id),
                ("scopes", SCOPES),
                ("device_code", &dev.device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| format!("Error al consultar el token: {e}"))?;

        if resp.status().is_success() {
            return resp
                .json::<TokenResponse>()
                .await
                .map_err(|e| format!("Token inválido: {e}"));
        }
        // Non-success while pending is expected (authorization_pending); keep polling.
    }
    Err("Tiempo de emparejamiento agotado. Probá de nuevo.".into())
}

async fn refresh(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    if refresh_token.is_empty() {
        return Err("sin refresh token".into());
    }
    client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<TokenResponse>()
        .await
        .map_err(|e| e.to_string())
}

// ── Helix: resolve the authenticated user (broadcaster) ──────────────────────

struct Broadcaster {
    id: String,
    login: String,
    display_name: String,
    avatar: String,
}

async fn get_broadcaster(
    client: &reqwest::Client,
    client_id: &str,
    access_token: &str,
) -> Result<Broadcaster, SessionError> {
    let resp = client
        .get(HELIX_USERS)
        .header("Client-Id", client_id)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| SessionError::Fatal(format!("Error al consultar el usuario: {e}")))?;

    if resp.status().as_u16() == 401 {
        return Err(SessionError::Auth);
    }
    if !resp.status().is_success() {
        return Err(SessionError::Fatal(format!(
            "Twitch /users devolvió {}",
            resp.status()
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| SessionError::Fatal(e.to_string()))?;
    let user = body
        .get("data")
        .and_then(|d| d.get(0))
        .ok_or_else(|| SessionError::Fatal("Respuesta de /users vacía".into()))?;
    let str_field = |k: &str| user.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SessionError::Fatal("Sin user id".into()))?
        .to_string();
    Ok(Broadcaster {
        id,
        login: str_field("login"),
        display_name: str_field("display_name"),
        avatar: str_field("profile_image_url"),
    })
}

// ── Channel Points rewards (for linking a reward to a real Twitch reward) ────

#[derive(Serialize)]
pub struct RewardInfo {
    pub id: String,
    pub title: String,
}

/// List the connected channel's Channel Points custom rewards. Returns ALL of
/// them (not only app-created — `only_manageable_rewards` defaults to false),
/// so the UI can link a reward to its exact Twitch title without typos.
pub async fn fetch_channel_rewards(data_dir: &Path) -> Result<Vec<RewardInfo>, String> {
    let tokens = load_tokens(data_dir);
    if tokens.access_token.trim().is_empty() {
        return Err("No estás conectado a Twitch.".into());
    }
    let client_id = effective_client_id(&tokens.client_id);
    let client = reqwest::Client::new();

    let bc = get_broadcaster(&client, &client_id, &tokens.access_token)
        .await
        .map_err(|e| match e {
            SessionError::Auth => "Sesión expirada. Reconectá con Twitch.".to_string(),
            SessionError::Fatal(m) => m,
            SessionError::Reconnect => "Error de conexión con Twitch.".to_string(),
        })?;

    let resp = client
        .get(HELIX_REWARDS)
        .header("Client-Id", &client_id)
        .bearer_auth(&tokens.access_token)
        .query(&[("broadcaster_id", bc.id.as_str())])
        .send()
        .await
        .map_err(|e| format!("Error al consultar rewards: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Twitch devolvió {} al listar rewards", resp.status()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out: Vec<RewardInfo> = vec![];
    if let Some(arr) = body.get("data").and_then(|d| d.as_array()) {
        for r in arr {
            let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
            if !title.is_empty() {
                out.push(RewardInfo {
                    id: r.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    title: title.to_string(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(out)
}

// ── EventSub: one WebSocket connection, processed until it drops ─────────────

async fn run_eventsub(
    client: &reqwest::Client,
    state: &AppState,
    tokens: &Tokens,
) -> Result<(), SessionError> {
    let shared = state.twitch.clone();

    let bc = get_broadcaster(client, &tokens.client_id, &tokens.access_token).await?;
    let broadcaster_id = bc.id.clone();
    let login = bc.login.clone();

    let (ws_stream, _) = tokio_tungstenite::connect_async(EVENTSUB_WS)
        .await
        .map_err(|e| SessionError::Fatal(format!("No se pudo abrir EventSub: {e}")))?;
    let (mut write, mut read) = ws_stream.split();

    // First message must be session_welcome; grab the session id + keepalive.
    let (session_id, keepalive) = {
        let mut found = None;
        for _ in 0..5 {
            match tokio::time::timeout(Duration::from_secs(15), read.next()).await {
                Ok(Some(Ok(Message::Text(txt)))) => {
                    let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or_default();
                    if v["metadata"]["message_type"] == "session_welcome" {
                        let id = v["payload"]["session"]["id"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let ka = v["payload"]["session"]["keepalive_timeout_seconds"]
                            .as_u64()
                            .unwrap_or(10);
                        found = Some((id, ka));
                        break;
                    }
                }
                Ok(Some(Ok(_))) => continue,
                _ => return Err(SessionError::Reconnect),
            }
        }
        found.ok_or(SessionError::Reconnect)?
    };

    // Subscribe to channel point redemptions over this WebSocket session.
    let sub_body = json!({
        "type": REDEMPTION_TYPE,
        "version": "1",
        "condition": { "broadcaster_user_id": broadcaster_id },
        "transport": { "method": "websocket", "session_id": session_id }
    });
    let sub_resp = client
        .post(HELIX_SUBS)
        .header("Client-Id", &tokens.client_id)
        .bearer_auth(&tokens.access_token)
        .json(&sub_body)
        .send()
        .await
        .map_err(|e| SessionError::Fatal(format!("Error al suscribir EventSub: {e}")))?;

    if sub_resp.status().as_u16() == 401 {
        return Err(SessionError::Auth);
    }
    if !sub_resp.status().is_success() {
        let code = sub_resp.status();
        let detail = sub_resp.text().await.unwrap_or_default();
        return Err(SessionError::Fatal(format!(
            "Suscripción rechazada ({code}): {detail}"
        )));
    }

    update_status(&shared, |s| {
        s.state = "connected".into();
        s.login = if login.is_empty() { None } else { Some(login.clone()) };
        s.display_name = Some(if bc.display_name.is_empty() {
            login.clone()
        } else {
            bc.display_name.clone()
        });
        s.avatar = if bc.avatar.is_empty() { None } else { Some(bc.avatar.clone()) };
        s.user_code = None;
        s.verification_uri = None;
        s.error = None;
    });
    println!("[Twitch] EventSub conectado como '{login}'. Escuchando canjes.");

    // Read loop. Twitch sends session_keepalive within `keepalive` seconds;
    // if nothing arrives within a small grace window, treat as dead → reconnect.
    let grace = Duration::from_secs(keepalive + 5);
    loop {
        match tokio::time::timeout(grace, read.next()).await {
            Err(_) => return Err(SessionError::Reconnect), // keepalive missed
            Ok(None) => return Err(SessionError::Reconnect), // stream closed
            Ok(Some(Err(_))) => return Err(SessionError::Reconnect),
            Ok(Some(Ok(msg))) => match msg {
                Message::Text(txt) => {
                    let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or_default();
                    match v["metadata"]["message_type"].as_str() {
                        Some("notification") => handle_notification(state, &v),
                        // Twitch is doing maintenance; reconnect from scratch.
                        Some("session_reconnect") => return Err(SessionError::Reconnect),
                        Some("revocation") => {
                            return Err(SessionError::Fatal(
                                "Twitch revocó la suscripción (¿se quitó el permiso?).".into(),
                            ));
                        }
                        _ => {} // session_keepalive, etc.
                    }
                }
                Message::Ping(data) => {
                    let _ = write.send(Message::Pong(data)).await;
                }
                Message::Close(_) => return Err(SessionError::Reconnect),
                _ => {}
            },
        }
    }
}

fn handle_notification(state: &AppState, v: &serde_json::Value) {
    let event = &v["payload"]["event"];
    let sub_type = v["payload"]["subscription"]["type"].as_str().unwrap_or("");
    if sub_type != REDEMPTION_TYPE {
        return;
    }
    let reward = event["reward"]["title"].as_str().unwrap_or("").to_string();
    let user = event["user_name"]
        .as_str()
        .or_else(|| event["user_login"].as_str())
        .unwrap_or("")
        .to_string();
    if reward.is_empty() {
        return;
    }
    let clients = crate::broadcast_play_video(&state.tx, &reward, &user);
    println!("[Twitch] Canje '{reward}' por {user} → overlays: {clients}");
}
