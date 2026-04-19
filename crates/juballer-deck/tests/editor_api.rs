//! Integration tests for the editor REST + WS surface.
//!
//! These tests exercise the in-process router (no socket bind) so they remain
//! parallel-safe and don't race over fixed ports.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use juballer_deck::config::{ConfigTree, DeckPaths};
use juballer_deck::editor::server::{EditorEvent, EditorServer, EditorState};
use std::path::PathBuf;
use std::sync::Arc;
use tower::util::ServiceExt;

fn fixture_paths() -> DeckPaths {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multipage");
    DeckPaths::from_root(path)
}

fn build_state(paths: DeckPaths, rt: &tokio::runtime::Handle) -> Arc<EditorState> {
    let cfg = ConfigTree::load(&paths).expect("load");
    let bus = juballer_deck::EventBus::default();
    let (bus_tx, _) = tokio::sync::broadcast::channel(64);
    Arc::new(EditorState {
        config: Arc::new(std::sync::Mutex::new(cfg)),
        paths,
        auth_token: None,
        plugin_host: None,
        rt: rt.clone(),
        deck_bus: bus,
        action_names: vec!["shell.run".to_string(), "deck.page_goto".to_string()],
        widget_names: vec!["clock".to_string()],
        plugin_names: vec![],
        action_schemas: Default::default(),
        widget_schemas: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "clock".to_string(),
                serde_json::json!({"type": "object", "properties": {"format": {"type": "string"}}}),
            );
            m
        },
        bus_tx,
    })
}

async fn body_json(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_state_returns_active_profile() {
    let rt = tokio::runtime::Handle::current();
    let state = build_state(fixture_paths(), &rt);
    let app = EditorServer::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp.into_body()).await;
    assert_eq!(v["active_profile"], "demo");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_action_schema_unknown_returns_404() {
    let rt = tokio::runtime::Handle::current();
    let state = build_state(fixture_paths(), &rt);
    let app = EditorServer::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/actions/nope/schema")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_widget_schema_returns_registered_schema() {
    let rt = tokio::runtime::Handle::current();
    let state = build_state(fixture_paths(), &rt);
    let app = EditorServer::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/widgets/clock/schema")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp.into_body()).await;
    assert_eq!(v["properties"]["format"]["type"], "string");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_action_schema_falls_back_to_empty_for_known_action() {
    let rt = tokio::runtime::Handle::current();
    let state = build_state(fixture_paths(), &rt);
    let app = EditorServer::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/actions/shell.run/schema")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp.into_body()).await;
    assert_eq!(v["type"], "object");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_page_writes_atomically_to_disk() {
    let rt = tokio::runtime::Handle::current();

    // Copy the multipage fixture into a tempdir so we don't pollute the source tree.
    let tmp = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multipage");
    copy_dir(&src, tmp.path()).unwrap();
    let paths = DeckPaths::from_root(tmp.path().to_path_buf());
    let state = build_state(paths.clone(), &rt);
    let app = EditorServer::router(state);

    let body = serde_json::json!({
        "meta": {"title": "newhome"},
        "button": [{
            "row": 0, "col": 0, "action": "shell.run", "args": {"cmd": "true"},
        }],
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/demo/pages/home")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp.into_body()).await;
    assert_eq!(v["ok"], true);

    // The file should now contain the new title.
    let on_disk = std::fs::read_to_string(paths.profile_page_toml("demo", "home")).unwrap();
    assert!(on_disk.contains("newhome"), "got:\n{on_disk}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_activate_unknown_profile_returns_404() {
    let rt = tokio::runtime::Handle::current();
    let tmp = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multipage");
    copy_dir(&src, tmp.path()).unwrap();
    let paths = DeckPaths::from_root(tmp.path().to_path_buf());
    let state = build_state(paths, &rt);
    let app = EditorServer::router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/no_such/activate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_activate_known_profile_rewrites_deck_toml() {
    let rt = tokio::runtime::Handle::current();
    let tmp = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multipage");
    copy_dir(&src, tmp.path()).unwrap();
    let paths = DeckPaths::from_root(tmp.path().to_path_buf());
    let state = build_state(paths.clone(), &rt);
    let app = EditorServer::router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/alt/activate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let on_disk = std::fs::read_to_string(&paths.deck_toml).unwrap();
    assert!(
        on_disk.contains(r#"active_profile = "alt""#),
        "got:\n{on_disk}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_restart_unknown_plugin_returns_404() {
    let rt = tokio::runtime::Handle::current();
    let state = build_state(fixture_paths(), &rt);
    let app = EditorServer::router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/plugins/whatever/restart")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_token_blocks_writes_when_set() {
    let rt = tokio::runtime::Handle::current();
    let tmp = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multipage");
    copy_dir(&src, tmp.path()).unwrap();
    let paths = DeckPaths::from_root(tmp.path().to_path_buf());
    let state = {
        let mut s = build_state(paths, &rt);
        Arc::get_mut(&mut s).unwrap().auth_token = Some("secret".into());
        s
    };
    let app = EditorServer::router(state);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/demo/activate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // With the right token it succeeds.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/demo/activate")
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_receives_profile_reloaded_event() {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message as TungMessage;

    let rt = tokio::runtime::Handle::current();
    let state = build_state(fixture_paths(), &rt);
    let bus_tx = state.bus_tx.clone();
    let app = EditorServer::router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://{bound}/ws");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&url).await.unwrap();

    // Give the server a moment to subscribe before we publish.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    bus_tx
        .send(EditorEvent::ProfileReloaded {
            profile: "alt".into(),
        })
        .unwrap();

    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
        .await
        .expect("ws timeout")
        .expect("ws closed")
        .expect("ws err");
    match msg {
        TungMessage::Text(t) => {
            assert!(t.contains(r#""kind":"profile_reloaded""#), "got: {t}");
            assert!(t.contains(r#""profile":"alt""#), "got: {t}");
        }
        other => panic!("unexpected ws msg: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_preview_action_publishes_to_deck_bus() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message as TungMessage;

    let rt = tokio::runtime::Handle::current();
    let state = build_state(fixture_paths(), &rt);
    let mut deck_bus_rx = state.deck_bus.subscribe();
    let app = EditorServer::router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("ws://{bound}/ws");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&url).await.unwrap();
    ws.send(TungMessage::Text(
        r#"{"kind":"preview_action","action":"shell.run","args":{"cmd":"true"}}"#.to_string(),
    ))
    .await
    .unwrap();

    let ev = tokio::time::timeout(std::time::Duration::from_secs(2), deck_bus_rx.recv())
        .await
        .expect("deck bus recv timeout")
        .expect("bus closed");
    assert_eq!(ev.topic, "widget.action_request");
    assert_eq!(ev.data["action"], "shell.run");
    assert_eq!(ev.data["args"]["cmd"], "true");
}

/// End-to-end: write a page through the editor's POST endpoint, then verify the file
/// watcher picks up the change and the editor's reload signal channel fires. We use the
/// existing `config::watch` helper directly (the cli.rs reload glue is integration-only).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_page_triggers_watcher_signal() {
    use juballer_deck::config::watch;

    let tmp = tempfile::tempdir().unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multipage");
    copy_dir(&src, tmp.path()).unwrap();

    let (_w, rx) =
        watch(tmp.path(), std::time::Duration::from_millis(150), Vec::new()).unwrap();

    let rt = tokio::runtime::Handle::current();
    let paths = DeckPaths::from_root(tmp.path().to_path_buf());
    let state = build_state(paths, &rt);
    let app = EditorServer::router(state);

    // Give the watcher a moment to settle before our write.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let body = serde_json::json!({
        "meta": {"title": "watched"},
        "button": [{"row": 0, "col": 0, "action": "shell.run", "args": {"cmd": "true"}}],
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/profiles/demo/pages/home")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The watcher must produce a reload signal within a few seconds.
    let got =
        tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(3)))
            .await
            .unwrap();
    assert!(got.is_ok(), "no reload signal");
}

/// Recursive directory copy. Tempfile + a real fixture is the cleanest way to test that
/// atomic writes land on disk without trampling the source tree.
fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &dst_path)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}
