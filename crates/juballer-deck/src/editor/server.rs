//! axum web editor — REST API + WebSocket for live sync + serves bundled SPA.
//!
//! Endpoints (all under `/api/v1`):
//!
//! Reads:
//!   GET  /state
//!   GET  /profiles
//!   GET  /profiles/:name
//!   GET  /actions
//!   GET  /widgets
//!   GET  /plugins
//!   GET  /actions/:name/schema
//!   GET  /widgets/:name/schema
//!
//! Writes (gated by [editor].require_auth bearer token if set):
//!   POST /profiles/:profile/pages/:page   — atomic write of a page TOML
//!   POST /profiles/:profile/activate      — flip deck.active_profile
//!   POST /plugins/:name/restart           — SIGTERM + respawn the plugin's child
//!
//! WS `/ws`:
//!   deck → editor: {"kind":"profile_reloaded"|"plugin_status"|"key_preview", ...}
//!   editor → deck: {"kind":"preview_action","action":"...","args":{...}}

use crate::bus::EventBus;
use crate::config::{atomic_write, ConfigTree, DeckPaths};
use crate::plugin::host::PluginHost;
use crate::Result;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Shared between axum handlers. Construct via [`EditorServer::new`] and let the server
/// task hold the only `Arc` after `spawn`; clone the `bus_tx` separately if the deck
/// needs to publish events outside the server.
pub struct EditorState {
    /// Snapshot of the deck's config at server-start. Reloaders may swap this in place.
    pub config: Arc<std::sync::Mutex<ConfigTree>>,
    /// On-disk paths for the config tree. Write endpoints compute file targets from this.
    pub paths: DeckPaths,
    /// Optional bearer token required on write endpoints. `None` ⇒ writes unauthenticated.
    pub auth_token: Option<String>,
    /// Shared handle to the plugin host. `None` if the deck was launched without plugins.
    pub plugin_host: Option<Arc<tokio::sync::Mutex<PluginHost>>>,
    /// Tokio runtime handle used to drive `restart_one` from axum handlers.
    pub rt: tokio::runtime::Handle,
    /// The deck's inner event bus. `preview_action` publishes a `widget.action_request`
    /// here so the existing render-loop path handles it uniformly.
    pub deck_bus: EventBus,
    pub action_names: Vec<String>,
    pub widget_names: Vec<String>,
    pub plugin_names: Vec<String>,
    /// Per-name JSON Schema for action args (populated from the registry at boot).
    /// Names not present here fall back to the empty placeholder.
    pub action_schemas: std::collections::HashMap<String, serde_json::Value>,
    /// Per-name JSON Schema for widget args.
    pub widget_schemas: std::collections::HashMap<String, serde_json::Value>,
    /// Broadcast channel for server → WS client push events.
    pub bus_tx: broadcast::Sender<EditorEvent>,
}

/// Events pushed from deck → editor WS clients. Serialized with a `kind` tag to match
/// the locked wire format in the spec.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorEvent {
    ProfileReloaded { profile: String },
    PluginStatus { name: String, status: String },
    KeyPreview { row: u8, col: u8, down: bool },
}

/// Editor → deck WS messages. Parsed with a `kind` tag.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ClientMessage {
    PreviewAction {
        action: String,
        #[serde(default)]
        args: serde_json::Value,
    },
}

pub struct EditorServer {
    pub bind: SocketAddr,
    pub state: Arc<EditorState>,
}

impl EditorServer {
    pub fn new(bind: SocketAddr, state: Arc<EditorState>) -> Self {
        Self { bind, state }
    }

    /// Build the router. Exposed so integration tests can drive the app without binding a port.
    pub fn router(state: Arc<EditorState>) -> Router {
        Router::new()
            .route("/", get(serve_index))
            .route("/api/v1/state", get(api_state))
            .route("/api/v1/profiles", get(api_profiles))
            .route("/api/v1/profiles/:name", get(api_profile))
            .route(
                "/api/v1/profiles/:profile/pages/:page",
                get(api_get_page).post(api_write_page),
            )
            .route(
                "/api/v1/profiles/:profile/activate",
                post(api_activate_profile),
            )
            .route("/api/v1/actions", get(api_actions))
            .route("/api/v1/actions/:name/schema", get(api_action_schema))
            .route("/api/v1/widgets", get(api_widgets))
            .route("/api/v1/widgets/:name/schema", get(api_widget_schema))
            .route("/api/v1/plugins", get(api_plugins))
            .route("/api/v1/plugins/:name/restart", post(api_restart_plugin))
            .route("/ws", get(ws_handler))
            .with_state(state)
    }

    /// Spawn the server on the given runtime. Returns immediately; the server runs forever.
    pub fn spawn(self, rt: &tokio::runtime::Handle) -> Result<()> {
        let bind = self.bind;
        let state = self.state.clone();
        rt.spawn(async move {
            let app = Self::router(state);
            tracing::info!("editor server listening on http://{}", bind);
            match tokio::net::TcpListener::bind(bind).await {
                Ok(listener) => {
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::warn!("editor server: {}", e);
                    }
                }
                Err(e) => tracing::warn!("editor bind failed: {}", e),
            }
        });
        Ok(())
    }
}

fn error_json(msg: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "error": msg.into() })
}

/// Reject URL segments that can escape the config root or poke at hidden
/// dotfiles before they ever reach a filesystem join. Names we accept map
/// 1:1 to on-disk directory / file names.
fn safe_segment(name: &str) -> std::result::Result<(), axum::response::Response> {
    let bad = name.is_empty()
        || name.len() > 64
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.starts_with('.');
    if bad {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error_json("invalid name: path-unsafe characters")),
        )
            .into_response());
    }
    Ok(())
}

/// Reject requests without the configured bearer token. `Ok(())` ⇒ allow; otherwise
/// returns a `(status, body)` pair the caller short-circuits with.
fn check_auth(
    state: &EditorState,
    headers: &HeaderMap,
) -> std::result::Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(expected) = state.auth_token.as_deref() else {
        return Ok(());
    };
    let got = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| headers.get("x-editor-token").and_then(|v| v.to_str().ok()));
    match got {
        Some(tok) if tok == expected => Ok(()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(error_json("missing or invalid auth token")),
        )),
    }
}

async fn serve_index() -> impl IntoResponse {
    Html(super::assets::INDEX_HTML.to_string())
}

async fn api_state(State(state): State<Arc<EditorState>>) -> impl IntoResponse {
    let cfg = state.config.lock().unwrap();
    Json(serde_json::json!({
        "active_profile": cfg.deck.active_profile,
        "profiles": cfg.profiles.keys().collect::<Vec<_>>(),
        "actions_count": state.action_names.len(),
        "widgets_count": state.widget_names.len(),
        "plugins_count": state.plugin_names.len(),
        "deck_version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn api_profiles(State(state): State<Arc<EditorState>>) -> impl IntoResponse {
    let cfg = state.config.lock().unwrap();
    let list: Vec<_> = cfg
        .profiles
        .iter()
        .map(|(name, p)| {
            serde_json::json!({
                "name": name,
                "description": p.meta.description,
                "default_page": p.meta.default_page,
                "pages": p.meta.pages,
            })
        })
        .collect();
    Json(list)
}

async fn api_profile(
    Path(name): Path<String>,
    State(state): State<Arc<EditorState>>,
) -> impl IntoResponse {
    if let Err(r) = safe_segment(&name) {
        return r;
    }
    let cfg = state.config.lock().unwrap();
    match cfg.profiles.get(&name) {
        Some(p) => {
            let pages: serde_json::Map<_, _> = p
                .pages
                .iter()
                .map(|(name, page)| (name.clone(), serde_json::to_value(page).unwrap()))
                .collect();
            Json(serde_json::json!({
                "meta": p.meta,
                "pages": pages,
            }))
            .into_response()
        }
        None => (StatusCode::NOT_FOUND, "profile not found").into_response(),
    }
}

async fn api_actions(State(state): State<Arc<EditorState>>) -> impl IntoResponse {
    Json(state.action_names.clone())
}

async fn api_widgets(State(state): State<Arc<EditorState>>) -> impl IntoResponse {
    Json(state.widget_names.clone())
}

async fn api_plugins(State(state): State<Arc<EditorState>>) -> impl IntoResponse {
    Json(state.plugin_names.clone())
}

async fn api_action_schema(
    Path(name): Path<String>,
    State(state): State<Arc<EditorState>>,
) -> impl IntoResponse {
    if let Err(r) = safe_segment(&name) {
        return r;
    }
    if !state.action_names.iter().any(|n| n == &name) {
        return (StatusCode::NOT_FOUND, Json(error_json("unknown action"))).into_response();
    }
    let s = state
        .action_schemas
        .get(&name)
        .cloned()
        .unwrap_or_else(super::schema::empty_schema);
    Json(s).into_response()
}

async fn api_widget_schema(
    Path(name): Path<String>,
    State(state): State<Arc<EditorState>>,
) -> impl IntoResponse {
    if let Err(r) = safe_segment(&name) {
        return r;
    }
    if !state.widget_names.iter().any(|n| n == &name) {
        return (StatusCode::NOT_FOUND, Json(error_json("unknown widget"))).into_response();
    }
    let s = state
        .widget_schemas
        .get(&name)
        .cloned()
        .unwrap_or_else(super::schema::empty_schema);
    Json(s).into_response()
}

async fn api_get_page(
    Path((profile, page)): Path<(String, String)>,
    State(state): State<Arc<EditorState>>,
) -> impl IntoResponse {
    if let Err(r) = safe_segment(&profile) {
        return r;
    }
    if let Err(r) = safe_segment(&page) {
        return r;
    }
    let cfg = state.config.lock().unwrap();
    let prof = match cfg.profiles.get(&profile) {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, "profile not found").into_response(),
    };
    match prof.pages.get(&page) {
        Some(p) => Json(serde_json::to_value(p).unwrap()).into_response(),
        None => (StatusCode::NOT_FOUND, "page not found").into_response(),
    }
}

async fn api_write_page(
    Path((profile, page)): Path<(String, String)>,
    State(state): State<Arc<EditorState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(r) = check_auth(&state, &headers) {
        return r.into_response();
    }
    if let Err(r) = safe_segment(&profile) {
        return r;
    }
    if let Err(r) = safe_segment(&page) {
        return r;
    }
    let toml_value = match json_to_toml_value(&body) {
        Some(v) => v,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error_json("body must be a TOML-compatible JSON object")),
            )
                .into_response();
        }
    };
    let serialized = match toml::to_string_pretty(&toml_value) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error_json(format!("toml encode: {e}"))),
            )
                .into_response();
        }
    };
    let dest = state.paths.profile_page_toml(&profile, &page);
    match atomic_write(&dest, serialized.as_bytes()) {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json(format!("write failed: {e}"))),
        )
            .into_response(),
    }
}

async fn api_activate_profile(
    Path(profile): Path<String>,
    State(state): State<Arc<EditorState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = check_auth(&state, &headers) {
        return r.into_response();
    }
    if let Err(r) = safe_segment(&profile) {
        return r;
    }
    {
        let cfg = state.config.lock().unwrap();
        if !cfg.profiles.contains_key(&profile) {
            return (StatusCode::NOT_FOUND, Json(error_json("profile not found"))).into_response();
        }
    }
    let deck_path = state.paths.deck_toml.clone();
    let current = match std::fs::read_to_string(&deck_path) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json(format!("read deck.toml: {e}"))),
            )
                .into_response();
        }
    };
    let mut doc: toml::Value = match toml::from_str(&current) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json(format!("parse deck.toml: {e}"))),
            )
                .into_response();
        }
    };
    if let Some(table) = doc.as_table_mut() {
        table.insert(
            "active_profile".into(),
            toml::Value::String(profile.clone()),
        );
    } else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json("deck.toml is not a table")),
        )
            .into_response();
    }
    let serialized = match toml::to_string_pretty(&doc) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_json(format!("toml encode: {e}"))),
            )
                .into_response();
        }
    };
    match atomic_write(&deck_path, serialized.as_bytes()) {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json(format!("write failed: {e}"))),
        )
            .into_response(),
    }
}

async fn api_restart_plugin(
    Path(name): Path<String>,
    State(state): State<Arc<EditorState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(r) = check_auth(&state, &headers) {
        return r.into_response();
    }
    if let Err(r) = safe_segment(&name) {
        return r;
    }
    let Some(host) = state.plugin_host.clone() else {
        return (StatusCode::NOT_FOUND, Json(error_json("plugin not found"))).into_response();
    };
    let rt = state.rt.clone();
    let mut guard = host.lock().await;
    match guard.restart_one(&name, &rt).await {
        Ok(true) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, Json(error_json("plugin not found"))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_json(format!("restart failed: {e}"))),
        )
            .into_response(),
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<EditorState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<EditorState>) {
    use futures_util::{SinkExt, StreamExt};

    let (mut sender, mut receiver) = socket.split();
    let mut bus_rx = state.bus_tx.subscribe();

    let send_task = tokio::spawn(async move {
        loop {
            match bus_rx.recv().await {
                Ok(ev) => {
                    if let Ok(s) = serde_json::to_string(&ev) {
                        if sender.send(WsMessage::Text(s)).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            WsMessage::Close(_) => break,
            WsMessage::Ping(_) => { /* axum handles pong */ }
            WsMessage::Text(txt) => match serde_json::from_str::<ClientMessage>(&txt) {
                Ok(ClientMessage::PreviewAction { action, args }) => {
                    // Republish as widget.action_request so the render loop drives it through
                    // the existing build + on_down path. Errors (unknown action, bad args)
                    // are tracing::warn'd by render, never propagated.
                    let payload = serde_json::json!({
                        "action": action,
                        "args": args,
                        "cell": [0, 0],
                    });
                    state.deck_bus.publish("widget.action_request", payload);
                }
                Err(e) => {
                    tracing::debug!("editor WS: invalid client message: {} ({})", e, txt);
                }
            },
            _ => {}
        }
    }
    send_task.abort();
}

/// Lossy JSON → TOML value converter. JSON nulls are dropped because TOML has no null type.
fn json_to_toml_value(v: &serde_json::Value) -> Option<toml::Value> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(toml::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml::Value::Integer(i))
            } else {
                n.as_f64().map(toml::Value::Float)
            }
        }
        serde_json::Value::String(s) => Some(toml::Value::String(s.clone())),
        serde_json::Value::Array(a) => Some(toml::Value::Array(
            a.iter().filter_map(json_to_toml_value).collect(),
        )),
        serde_json::Value::Object(m) => {
            let mut t = toml::Table::new();
            for (k, v) in m {
                if let Some(tv) = json_to_toml_value(v) {
                    t.insert(k.clone(), tv);
                }
            }
            Some(toml::Value::Table(t))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_segment_accepts_plain_names() {
        assert!(safe_segment("home").is_ok());
        assert!(safe_segment("main_page-01").is_ok());
    }

    #[test]
    fn safe_segment_rejects_traversal_and_separators() {
        for bad in ["..", ".", "../x", "x/y", "x\\y", ".hidden", "", "a\0b"] {
            assert!(safe_segment(bad).is_err(), "expected reject for {bad:?}");
        }
    }

    #[test]
    fn client_message_parses_preview_action() {
        let raw = r#"{"kind":"preview_action","action":"shell.run","args":{"cmd":"true"}}"#;
        let m: ClientMessage = serde_json::from_str(raw).unwrap();
        match m {
            ClientMessage::PreviewAction { action, args } => {
                assert_eq!(action, "shell.run");
                assert_eq!(args["cmd"], "true");
            }
        }
    }

    #[test]
    fn editor_event_serializes_with_kind() {
        let ev = EditorEvent::ProfileReloaded {
            profile: "homelab".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains(r#""kind":"profile_reloaded""#));
        assert!(s.contains(r#""profile":"homelab""#));

        let ev = EditorEvent::KeyPreview {
            row: 1,
            col: 2,
            down: true,
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains(r#""kind":"key_preview""#));
        assert!(s.contains(r#""row":1"#));
        assert!(s.contains(r#""down":true"#));
    }

    #[test]
    fn json_to_toml_drops_nulls() {
        let v: serde_json::Value = serde_json::json!({
            "x": 1,
            "y": null,
            "nested": {"a": "b", "c": null},
        });
        let t = json_to_toml_value(&v).unwrap();
        let out = toml::to_string(&t).unwrap();
        assert!(out.contains("x = 1"));
        assert!(!out.contains("y"));
        assert!(out.contains("a = \"b\""));
    }
}
