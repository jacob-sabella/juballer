//! Plugin host — spawns + supervises plugin processes, drives the UDS NDJSON protocol.
//!
//! For each plugin manifest in `plugins_dir`:
//! - create a UDS socket at `${XDG_RUNTIME_DIR:-/tmp}/juballer/plugins/<name>.sock`
//! - spawn the plugin's `entry_point` with env JUBALLER_SOCK + JUBALLER_PLUGIN_NAME + JUBALLER_PROTOCOL_VERSION
//! - accept the inbound connection, exchange `hello`
//! - run a tokio task that reads NDJSON Messages from the plugin and forwards them to a receiver channel
//! - hold a sender for outbound Messages (deck → plugin)
//!
//! Auto-restart on crash is not yet implemented; the editor can restart on demand
//! via [`PluginHost::restart_one`].

use crate::app::NamedTileOverride;
use crate::Result;
use juballer_deck_protocol::view::ViewNode;
use juballer_deck_protocol::{Message, PROTOCOL_VERSION};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

pub type ViewTreeStore = Arc<RwLock<HashMap<String, ViewNode>>>;
pub type NamedTileStore = Arc<StdMutex<HashMap<String, NamedTileOverride>>>;

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

pub struct PluginHost {
    pub plugins_dir: PathBuf,
    pub plugins: HashMap<String, PluginConn>,
    /// Shared view-tree store. Captured on first `spawn_all` so subsequent
    /// `restart_one` calls can re-wire the plugin's read task without plumbing
    /// the store through every call-site.
    pub view_trees: Option<ViewTreeStore>,
    /// Shared named-tile override store. Plugins write via `Message::TileSetByName`;
    /// the render loop applies these to `app.tiles` each frame.
    pub named_tiles: Option<NamedTileStore>,
    /// Optional sink for plugin lifecycle events (spawn / crash / restart).
    /// The editor server subscribes to these so WS clients can show status.
    pub status_tx: Option<tokio::sync::broadcast::Sender<PluginStatusEvent>>,
}

/// Plugin lifecycle event used by the editor WS bus.
#[derive(Debug, Clone)]
pub struct PluginStatusEvent {
    pub name: String,
    pub status: PluginStatus,
}

#[derive(Debug, Clone, Copy)]
pub enum PluginStatus {
    Ok,
    Crashed,
    Restarting,
}

impl PluginStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginStatus::Ok => "ok",
            PluginStatus::Crashed => "crashed",
            PluginStatus::Restarting => "restarting",
        }
    }
}

pub struct PluginConn {
    pub manifest: super::manifest::PluginManifest,
    pub send: tokio::sync::mpsc::Sender<Message>,
    pub recv: Arc<Mutex<tokio::sync::mpsc::Receiver<Message>>>,
    #[cfg(unix)]
    _child: tokio::process::Child,
}

impl PluginHost {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            plugins_dir,
            plugins: HashMap::new(),
            view_trees: None,
            named_tiles: None,
            status_tx: None,
        }
    }

    /// Install a status event sender. Plugin spawn/restart calls publish on it.
    pub fn set_status_tx(&mut self, tx: tokio::sync::broadcast::Sender<PluginStatusEvent>) {
        self.status_tx = Some(tx);
    }

    fn emit_status(&self, name: &str, status: PluginStatus) {
        if let Some(tx) = &self.status_tx {
            let _ = tx.send(PluginStatusEvent {
                name: name.to_string(),
                status,
            });
        }
    }

    /// Discover all plugins in plugins_dir without spawning.
    pub fn discover(&self) -> Result<Vec<super::manifest::PluginManifest>> {
        let mut out = Vec::new();
        if !self.plugins_dir.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(&self.plugins_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let manifest_path = entry.path().join("manifest.toml");
            if manifest_path.exists() {
                match super::manifest::PluginManifest::load(&manifest_path) {
                    Ok(m) => out.push(m),
                    Err(e) => tracing::warn!("plugin {:?} manifest invalid: {}", entry.path(), e),
                }
            }
        }
        Ok(out)
    }

    /// Spawn all discovered plugins on the given runtime. Returns when all plugins
    /// are spawned (their connection accept tasks may still be in flight).
    #[cfg(unix)]
    pub async fn spawn_all(
        &mut self,
        rt: &tokio::runtime::Handle,
        view_trees: ViewTreeStore,
        named_tiles: NamedTileStore,
    ) -> Result<()> {
        self.view_trees = Some(view_trees.clone());
        self.named_tiles = Some(named_tiles.clone());
        let manifests = self.discover()?;
        for manifest in manifests {
            if let Err(e) = self
                .spawn_one(&manifest, rt, view_trees.clone(), named_tiles.clone())
                .await
            {
                tracing::warn!("spawn plugin {}: {}", manifest.name, e);
            }
        }
        Ok(())
    }

    /// Restart a single plugin by name: drop its existing connection (which kills
    /// the child via `kill_on_drop`) and spawn a fresh instance using the same
    /// manifest. Returns `Ok(true)` on success, `Ok(false)` if no plugin by that
    /// name is known.
    #[cfg(unix)]
    pub async fn restart_one(&mut self, name: &str, rt: &tokio::runtime::Handle) -> Result<bool> {
        let manifest = match self.plugins.get(name) {
            Some(conn) => conn.manifest.clone(),
            None => return Ok(false),
        };
        let view_trees = self
            .view_trees
            .clone()
            .ok_or_else(|| crate::Error::Config("plugin host has no view_trees".into()))?;
        let named_tiles = self
            .named_tiles
            .clone()
            .ok_or_else(|| crate::Error::Config("plugin host has no named_tiles".into()))?;

        self.emit_status(name, PluginStatus::Restarting);
        // Drop the old connection. `kill_on_drop` on the child + closing the
        // send channel tears the plugin down.
        self.plugins.remove(name);

        self.spawn_one(&manifest, rt, view_trees, named_tiles)
            .await?;
        self.emit_status(name, PluginStatus::Ok);
        Ok(true)
    }

    #[cfg(unix)]
    async fn spawn_one(
        &mut self,
        manifest: &super::manifest::PluginManifest,
        _rt: &tokio::runtime::Handle,
        view_trees: ViewTreeStore,
        named_tiles: NamedTileStore,
    ) -> Result<()> {
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let sock_dir = runtime_dir.join("juballer").join("plugins");
        std::fs::create_dir_all(&sock_dir)?;
        let sock_path = sock_dir.join(format!("{}.sock", manifest.name));
        let _ = std::fs::remove_file(&sock_path);

        let listener = UnixListener::bind(&sock_path)?;

        let entry = self
            .plugins_dir
            .join(&manifest.name)
            .join(&manifest.entry_point);
        let cwd = self.plugins_dir.join(&manifest.name);

        let mut cmd = match manifest.language.as_str() {
            "python" => {
                let mut c = tokio::process::Command::new("python3");
                c.arg(&entry);
                c
            }
            _ => tokio::process::Command::new(&entry),
        };
        cmd.current_dir(&cwd)
            .env("JUBALLER_SOCK", &sock_path)
            .env("JUBALLER_PLUGIN_NAME", &manifest.name)
            .env("JUBALLER_PROTOCOL_VERSION", PROTOCOL_VERSION.to_string());

        cmd.kill_on_drop(true);
        #[cfg(target_os = "linux")]
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        tracing::info!(
            "spawned plugin '{}' pid={:?} (socket: {:?})",
            manifest.name,
            child.id(),
            sock_path
        );
        self.emit_status(&manifest.name, PluginStatus::Ok);

        let (in_tx, in_rx) = tokio::sync::mpsc::channel::<Message>(64);
        let (out_tx, out_rx) = tokio::sync::mpsc::channel::<Message>(64);

        let plugin_name = manifest.name.clone();
        let view_trees_for_conn = view_trees.clone();
        let named_tiles_for_conn = named_tiles.clone();
        tokio::spawn(async move {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    tracing::info!("plugin '{}' connected", plugin_name);
                    if let Err(e) = run_connection(
                        stream,
                        in_tx,
                        out_rx,
                        view_trees_for_conn,
                        named_tiles_for_conn,
                    )
                    .await
                    {
                        tracing::warn!("plugin '{}' connection error: {}", plugin_name, e);
                    }
                }
                Err(e) => {
                    tracing::warn!("plugin '{}' accept failed: {}", plugin_name, e);
                }
            }
        });

        self.plugins.insert(
            manifest.name.clone(),
            PluginConn {
                manifest: manifest.clone(),
                send: out_tx,
                recv: Arc::new(Mutex::new(in_rx)),
                _child: child,
            },
        );
        Ok(())
    }

    #[cfg(not(unix))]
    pub async fn spawn_all(
        &mut self,
        _rt: &tokio::runtime::Handle,
        _view_trees: ViewTreeStore,
        _named_tiles: NamedTileStore,
    ) -> Result<()> {
        tracing::warn!("plugin host: non-unix platforms not supported");
        Ok(())
    }
}

#[cfg(unix)]
async fn run_connection(
    stream: UnixStream,
    in_tx: tokio::sync::mpsc::Sender<Message>,
    mut out_rx: tokio::sync::mpsc::Receiver<Message>,
    view_trees: ViewTreeStore,
    named_tiles: NamedTileStore,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();

    // Send initial Hello.
    let hello = Message::Hello {
        v: PROTOCOL_VERSION,
        deck_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        plugin: None,
        plugin_version: None,
        sdk: None,
    };
    write_message(&mut write, &hello).await?;
    write_message(&mut write, &Message::RegisterComplete).await?;

    let read_task: tokio::task::JoinHandle<std::io::Result<()>> = tokio::spawn(async move {
        let mut reader = BufReader::new(read);
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim();
            if let Some((pane, tree)) = parse_view_update_envelope(trimmed) {
                tracing::debug!("plugin view_update: pane={}", pane);
                if let Ok(mut store) = view_trees.write() {
                    store.insert(pane, tree);
                }
                continue;
            }
            match serde_json::from_str::<Message>(trimmed) {
                Ok(Message::WidgetViewUpdate { pane, tree }) => {
                    if let Ok(mut store) = view_trees.write() {
                        store.insert(pane, tree);
                    }
                }
                Ok(Message::TileSetByName {
                    name,
                    icon,
                    label,
                    state_color,
                    clear,
                }) => {
                    apply_tile_set_by_name(&named_tiles, name, icon, label, state_color, clear);
                }
                Ok(m) => {
                    if in_tx.send(m).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!("plugin sent invalid NDJSON: {} ({})", e, trimmed),
            }
        }
        Ok(())
    });

    while let Some(m) = out_rx.recv().await {
        if let Err(e) = write_message(&mut write, &m).await {
            tracing::warn!("plugin write failed: {}", e);
            break;
        }
    }

    let _ = read_task.await;
    Ok(())
}

/// Merge a `TileSetByName` message into the shared override store. `clear=true`
/// removes the entry entirely (restoring config defaults at paint time); omitted
/// fields preserve the current plugin-override value so partial updates chain.
fn apply_tile_set_by_name(
    store: &NamedTileStore,
    name: String,
    icon: Option<String>,
    label: Option<String>,
    state_color: Option<String>,
    clear: Option<bool>,
) {
    let Ok(mut map) = store.lock() else {
        return;
    };
    if clear.unwrap_or(false) {
        map.remove(&name);
        return;
    }
    let entry = map.entry(name).or_default();
    if let Some(i) = icon {
        entry.icon = Some(i);
    }
    if let Some(l) = label {
        entry.label = Some(l);
    }
    if let Some(s) = state_color {
        entry.state_color = crate::theme::parse_named_color_core(&s);
    }
}

/// Parse the `{"kind":"widget.view_update","pane":...,"tree":...}` envelope used by
/// the locked plugin → deck view-tree wire format. Returns `None` if the line is not a
/// view_update envelope (caller should then fall back to `Message` parsing).
fn parse_view_update_envelope(line: &str) -> Option<(String, ViewNode)> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let obj = v.as_object()?;
    if obj.get("kind").and_then(|k| k.as_str())? != "widget.view_update" {
        return None;
    }
    let pane = obj.get("pane")?.as_str()?.to_string();
    let tree: ViewNode = serde_json::from_value(obj.get("tree")?.clone()).ok()?;
    Some((pane, tree))
}

#[cfg(unix)]
async fn write_message(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    m: &Message,
) -> std::io::Result<()> {
    let mut s = serde_json::to_string(m)?;
    s.push('\n');
    write.write_all(s.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locked_view_update_envelope() {
        let line = r#"{"kind":"widget.view_update","pane":"discord_pane","tree":{"kind":"text","value":"hi"}}"#;
        let (pane, tree) = parse_view_update_envelope(line).unwrap();
        assert_eq!(pane, "discord_pane");
        match tree {
            ViewNode::Text { value, .. } => assert_eq!(value, "hi"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn apply_tile_set_by_name_merges_partial_updates() {
        let store: NamedTileStore = Arc::new(StdMutex::new(HashMap::new()));
        apply_tile_set_by_name(
            &store,
            "discord_unread".into(),
            Some("💬".into()),
            Some("3".into()),
            Some("red".into()),
            None,
        );
        {
            let g = store.lock().unwrap();
            let ov = g.get("discord_unread").unwrap();
            assert_eq!(ov.icon.as_deref(), Some("💬"));
            assert_eq!(ov.label.as_deref(), Some("3"));
            assert!(ov.state_color.is_some());
        }
        // Partial update: label only — icon + color preserved.
        apply_tile_set_by_name(
            &store,
            "discord_unread".into(),
            None,
            Some("7".into()),
            None,
            None,
        );
        {
            let g = store.lock().unwrap();
            let ov = g.get("discord_unread").unwrap();
            assert_eq!(ov.icon.as_deref(), Some("💬"));
            assert_eq!(ov.label.as_deref(), Some("7"));
            assert!(ov.state_color.is_some());
        }
        // Clear resets.
        apply_tile_set_by_name(
            &store,
            "discord_unread".into(),
            None,
            None,
            None,
            Some(true),
        );
        assert!(store.lock().unwrap().get("discord_unread").is_none());
    }

    #[test]
    fn rejects_non_view_update_lines() {
        assert!(parse_view_update_envelope(r#"{"type":"ping"}"#).is_none());
        assert!(parse_view_update_envelope(r#"{"kind":"text","value":"hi"}"#).is_none());
        assert!(parse_view_update_envelope("not json").is_none());
    }
}
