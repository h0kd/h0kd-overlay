use crate::logln;
use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tower_http::{cors::CorsLayer, services::ServeDir};

/// Local HTTP/WS port. The experimental build owns 3002 so it can run side
/// by side with the stable app (which keeps 3001) instead of dying on
/// AddrInUse. Single source of truth: the control panel asks for it via
/// `server_url`, and the overlay derives it from its own location.
pub const SERVER_PORT: u16 = 3002;

static OVERLAY_HTML: &str = include_str!("../../src/overlay.html");

// ── Preview (ventana de la app, para VRChat) ─────────────────────────────────
// La ventana de preview es un ESPEJO mudo de lo que el overlay de OBS está
// reproduciendo. No puede colgarse del mismo broadcast que el overlay: ahí
// `receiver_count()` significa "hay un overlay", y con la preview conectada
// el agente largaría pedidos sin OBS y nadie mandaría el `requestEnded`. Por
// eso tiene su propio canal, en el que solo entra lo que ya pasó por el
// overlay, y jamás se le lee nada.

/// Qué hay en pantalla ahora, para que una preview que se abre a mitad de un
/// pedido arranque desde donde va y no desde cero.
#[derive(Clone)]
pub struct PreviewHub {
    tx: tokio::sync::broadcast::Sender<String>,
    now: std::sync::Arc<std::sync::Mutex<Option<(serde_json::Value, std::time::Instant)>>>,
}

impl PreviewHub {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel::<String>(16);
        PreviewHub { tx, now: std::sync::Arc::new(std::sync::Mutex::new(None)) }
    }

    /// El agente mandó `playRequest` al overlay (y había overlay). `data` es
    /// el mismo objeto que recibió el overlay: itemId, src, title, submitter.
    pub fn play(&self, data: &serde_json::Value) {
        if let Ok(mut n) = self.now.lock() {
            *n = Some((data.clone(), std::time::Instant::now()));
        }
        let _ = self.tx.send(json!({ "type": "play", "item": data }).to_string());
    }

    /// El overlay terminó (o se fue con) ese pedido.
    pub fn end(&self, item_id: &str) {
        if let Ok(mut n) = self.now.lock() {
            if n.as_ref().map(|(d, _)| d["itemId"].as_str() == Some(item_id)).unwrap_or(false) {
                *n = None;
            }
        }
        let _ = self.tx.send(json!({ "type": "end", "itemId": item_id }).to_string());
    }

    fn hello(&self) -> String {
        let now = self.now.lock().ok().and_then(|n| n.clone()).map(|(mut d, since)| {
            d["elapsed"] = json!(since.elapsed().as_secs_f64());
            d
        });
        json!({ "type": "hello", "now": now }).to_string()
    }
}

async fn preview_ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let hub = state.preview.clone();
    ws.on_upgrade(move |socket| handle_preview_ws(socket, hub))
}

async fn handle_preview_ws(socket: WebSocket, hub: PreviewHub) {
    let (mut sink, mut stream) = socket.split();
    let mut rx = hub.tx.subscribe();
    if sink.send(Message::Text(hub.hello())).await.is_err() {
        return;
    }
    logln!("[WS] Preview conectada.");

    // Lo que diga la preview se descarta: no arbitra nada.
    let mut read = tokio::spawn(async move {
        while let Some(msg) = stream.next().await {
            if matches!(msg, Ok(Message::Close(_)) | Err(_)) {
                break;
            }
        }
    });
    let mut write = tokio::spawn(async move {
        let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(10));
        keepalive.tick().await;
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(m) => if sink.send(Message::Text(m)).await.is_err() { break; },
                    Err(_) => break,
                },
                _ = keepalive.tick() => {
                    if sink.send(Message::Ping(Vec::new())).await.is_err() { break; }
                }
            }
        }
    });
    tokio::select! {
        _ = &mut read  => { write.abort(); }
        _ = &mut write => { read.abort();  }
    }
    logln!("[WS] Preview desconectada.");
}

/// Identifica ESTA ejecución de la app. Va en el saludo del WebSocket: si el
/// overlay ve un valor distinto al de su primera conexión, la app se reinició
/// (quizá con un build nuevo) y se recarga solo. Sin esto, la fuente de
/// navegador de OBS seguía corriendo el HTML viejo después de actualizar la
/// app: se reconectaba por WS, cargaba la config nueva, y la aplicaba con el
/// código de antes (el volumen de los pedidos, por ejemplo, no cambiaba).
fn boot_id() -> u64 {
    static BOOT: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BOOT.get_or_init(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1)
    })
}

pub async fn start(state: AppState) -> std::io::Result<()> {
    let videos_dir = state.data_dir.join("videos");
    let health = state.server_health.clone();

    // Los mp4 de los pedidos se sirven desde una carpeta APARTE de videos/.
    // Mezclarlos haría que aparezcan en el desplegable de rewards del panel,
    // que lista esa carpeta entera.
    let mut app = Router::new()
        .route("/", get(root_handler))
        .route("/overlay", get(serve_overlay))
        .route("/config.json", get(serve_config_json))
        .route("/api/config", get(get_config_handler).post(post_config_handler))
        .route("/api/videos", get(list_videos_handler))
        .nest_service("/videos", ServeDir::new(videos_dir));

    if state.video_requests_enabled {
        let media = crate::video_requests::media_dir(&state.data_dir);
        let _ = std::fs::create_dir_all(&media);
        app = app
            .nest_service("/video-requests", ServeDir::new(media))
            // WS de la ventana de preview (ver PreviewHub). La página en sí va
            // empaquetada en la app; acá solo vive el espejo.
            .route("/preview", get(preview_ws_handler));
    }

    let app = app.layer(CorsLayer::permissive()).with_state(state);

    let bind = format!("127.0.0.1:{}", SERVER_PORT);
    // `?` propagates AddrInUse to the caller, which records it as ServerHealth::Error.
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    if let Ok(mut h) = health.lock() {
        *h = crate::ServerHealth::Ok;
    }
    logln!("[Server] Listening on http://{}", bind);
    logln!("[Server] Overlay (OBS) → http://{}/overlay", bind);

    axum::serve(listener, app)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(())
}

// ── Root: WS upgrade for overlay, status page for browsers ───────────────────

async fn root_handler(State(state): State<AppState>, ws: Option<WebSocketUpgrade>) -> Response {
    match ws {
        Some(upgrade) => {
            let tx = state.tx.clone();
            let vr = state.vr_cmd.clone();
            let preview = state.preview.clone();
            upgrade.on_upgrade(move |socket| handle_ws(socket, tx, vr, preview))
        }
        None => Html(
            r#"<!doctype html><meta charset="utf-8"><title>Stream Overlay</title>
            <body style="background:#0e0e10;color:#efeff1;font-family:system-ui;padding:40px">
            <h1>Stream Overlay</h1>
            <p>El servidor está corriendo. Endpoints:</p>
            <ul>
              <li><a href="/overlay" style="color:#9147ff">/overlay</a> — pegar en OBS Browser Source</li>
            </ul>
            <p>El panel de control está en la ventana principal de la app.</p>
            </body>"#,
        )
        .into_response(),
    }
}

async fn handle_ws(
    socket: WebSocket,
    tx: tokio::sync::broadcast::Sender<String>,
    vr: Option<tokio::sync::mpsc::Sender<crate::video_requests::VrCmd>>,
    preview: PreviewHub,
) {
    let (mut sink, mut stream) = socket.split();
    let mut rx = tx.subscribe();

    // Send Connected event (matches original protocol)
    let hello = json!({
        "event": { "source": "System", "type": "Connected" },
        "data": { "clients": tx.receiver_count(), "boot": boot_id() }
    })
    .to_string();
    let _ = sink.send(Message::Text(hello)).await;
    logln!("[WS] Overlay conectado. Total: {}", tx.receiver_count());

    // Reader: drain incoming messages and notice when the overlay goes away.
    // Breaking on Close (or any error) lets us reap the connection promptly so
    // the receiver count stays accurate for the panel's "overlay connected"
    // indicator.
    let mut read = tokio::spawn(async move {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                // El overlay avisa qué está mostrando. Es lo que le permite al
                // agente saber cuándo la pantalla quedó libre: sin esto, el
                // arbitraje tendría que adivinar por duración.
                Ok(Message::Text(txt)) => {
                    if let Some(vr) = vr.as_ref() {
                        if let Some(ev) = parse_overlay_event(&txt) {
                            // La preview se apaga cuando el overlay REAL
                            // termina, no cuando ella cree que terminó.
                            if let crate::video_requests::OverlayEvent::RequestEnded { item_id, .. } = &ev {
                                preview.end(item_id);
                            }
                            let _ = vr.send(crate::video_requests::VrCmd::Overlay(ev)).await;
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // Writer: forward broadcast messages, plus a keepalive ping so a dead
    // overlay is detected even when no redemptions are flowing (otherwise a
    // crashed OBS would linger in the count until the next broadcast).
    let mut write = tokio::spawn(async move {
        let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(10));
        keepalive.tick().await; // first tick fires immediately — skip it
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(m) => if sink.send(Message::Text(m)).await.is_err() { break; },
                    Err(_) => break, // channel closed or lagged
                },
                _ = keepalive.tick() => {
                    if sink.send(Message::Ping(Vec::new())).await.is_err() { break; }
                }
            }
        }
    });

    // When either half finishes (overlay closed, send failed, ping failed),
    // abort the other so its `rx` subscription is dropped immediately. Without
    // this the writer would keep its subscription alive, inflating the count.
    tokio::select! {
        _ = &mut read  => { write.abort(); }
        _ = &mut write => { read.abort();  }
    }
    logln!("[WS] Overlay desconectado. Total: {}", tx.receiver_count().saturating_sub(1));
}

/// Traduce lo que manda el overlay. Devuelve `None` para cualquier cosa que
/// no se reconozca: un mensaje raro del navegador no puede tumbar nada.
fn parse_overlay_event(txt: &str) -> Option<crate::video_requests::OverlayEvent> {
    use crate::video_requests::OverlayEvent;
    let v: serde_json::Value = serde_json::from_str(txt).ok()?;
    match v["type"].as_str()? {
        "overlayBusy" => Some(OverlayEvent::Busy(v["busy"].as_bool()?)),
        "requestStarted" => Some(OverlayEvent::RequestStarted(v["itemId"].as_str()?.to_string())),
        "requestEnded" => Some(OverlayEvent::RequestEnded {
            item_id: v["itemId"].as_str()?.to_string(),
            seconds: v["seconds"].as_f64().unwrap_or(0.0),
            // Si el overlay no lo dice, se asume que falló: dar por reproducido
            // algo que quizás nadie vio es la mentira más cara de las dos.
            reason: v["reason"].as_str().unwrap_or("error").to_string(),
        }),
        _ => None,
    }
}

// ── /overlay ────────────────────────────────────────────────────────────────

async fn serve_overlay() -> impl IntoResponse {
    Html(OVERLAY_HTML)
}

// ── /config.json (overlay fetches this) ─────────────────────────────────────

async fn serve_config_json(State(state): State<AppState>) -> Response {
    match std::fs::read_to_string(state.data_dir.join("config.json")) {
        Ok(s) => ([(header::CONTENT_TYPE, "application/json")], s).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            "{}",
        )
            .into_response(),
    }
}

// ── /api/config (admin UI — kept HTTP for compat / external tools) ──────────

async fn get_config_handler(State(state): State<AppState>) -> Response {
    let path = state.data_dir.join("config.json");
    let value: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| json!({
            "rewards": {},
            "safeZones": { "exclude": [] },
            "canvas": { "width": 1920, "height": 1080 }
        })),
        Err(_) => json!({
            "rewards": {},
            "safeZones": { "exclude": [] },
            "canvas": { "width": 1920, "height": 1080 }
        }),
    };
    Json(value).into_response()
}

async fn post_config_handler(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let pretty = match serde_json::to_string_pretty(&body) {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() })))
                .into_response();
        }
    };
    match std::fs::write(state.data_dir.join("config.json"), pretty) {
        Ok(_) => {
            // Live-reload connected overlays (same as the Tauri save command).
            let _ = state.tx.send(crate::reload_config_msg());
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ── /api/videos ─────────────────────────────────────────────────────────────

async fn list_videos_handler(State(state): State<AppState>) -> Response {
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
    Json(files).into_response()
}

