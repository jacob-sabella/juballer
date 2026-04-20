//! Capture current Carla parameter values into a preset file.
//!
//! Phase 5 ties together the [`super::listener`] read path and the
//! [`super::config::Preset`] schema: spin up the listener for a short
//! window so Carla pushes whatever it has, snapshot the feed, and
//! write the result as a preset TOML the user can later trigger via
//! `load-preset` or `open-preset-picker`.
//!
//! Caveat: Carla 2.6 only emits `/Carla/param` for parameters that
//! have changed since startup, so the captured snapshot may be a
//! subset of every parameter the plugin defines. The CLI logs which
//! indices it captured so the operator can spot a thin snapshot;
//! a follow-up phase will fill in defaults from a parsed `*.carxp`.

use crate::carla::config::{ParamRef, PluginRef, Preset, PresetParam};
use crate::carla::listener::{self, CarlaFeed};
use crate::carla::names::NameMap;
use crate::Result;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// What the operator asks for. CLI translates flags into this struct.
#[derive(Debug, Clone)]
pub struct CaptureRequest {
    /// Carla OSC server.
    pub target: SocketAddr,
    /// Plugin to snapshot — name (resolved via the optional name map)
    /// or numeric index.
    pub plugin: PluginRef,
    /// User-facing preset name; written into the preset file as `name`
    /// and used for the `<name>.preset.toml` filename.
    pub name: String,
    /// Optional description copied into the preset file.
    pub description: Option<String>,
    /// Optional category — translates to the parent dir under `root`.
    /// `None` writes the file directly under `root`.
    pub category: Option<String>,
    /// Preset library root. Defaults to
    /// `~/.config/juballer/carla/presets/` at the CLI layer.
    pub root: PathBuf,
    /// How long to wait for Carla to push parameter updates before
    /// snapshotting. Carla broadcasts continuously, but values that
    /// haven't changed since startup may not be pushed until something
    /// touches them.
    pub capture_window: Duration,
    /// `target_plugin` field written into the preset. Defaults to the
    /// resolved plugin's display name when `None`.
    pub target_plugin_label: Option<String>,
}

impl CaptureRequest {
    /// Default capture window. Long enough for Carla's typical pre-roll
    /// burst (peaks + initial param sweep) without making the CLI
    /// feel laggy.
    pub fn default_capture_window() -> Duration {
        Duration::from_millis(2_500)
    }
}

/// What `capture_preset` produced. Mostly mirrors the file contents +
/// where it landed; the caller decides how to surface this to the user.
#[derive(Debug, Clone)]
pub struct CaptureReport {
    pub written_to: PathBuf,
    pub preset: Preset,
    pub plugin_index: u32,
    /// True when the listener saw at least one OSC packet from Carla.
    /// `false` means the snapshot is empty and probably wrong; the CLI
    /// logs a warning so the operator notices.
    pub feed_was_active: bool,
}

/// Snapshot the live Carla feed for one plugin and write the result
/// as a preset TOML. Builds its own short-lived tokio runtime so the
/// CLI can call this without standing one up.
pub fn capture_preset(req: CaptureRequest, names: &NameMap) -> Result<CaptureReport> {
    let plugin_index = names.resolve_plugin(&req.plugin).ok_or_else(|| {
        crate::Error::Config(format!(
            "plugin {:?} not in name map; pass a numeric index or use --project",
            req.plugin
        ))
    })?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| crate::Error::Config(format!("tokio runtime: {e}")))?;
    let listener = listener::spawn(rt.handle(), req.target)?;
    let feed = listener.feed();
    rt.block_on(async {
        tokio::time::sleep(req.capture_window).await;
    });
    let snapshot = read_feed(&feed);
    listener.shutdown();
    // Give the listener task a moment to send /unregister before the
    // runtime drops.
    rt.block_on(async {
        tokio::time::sleep(Duration::from_millis(150)).await;
    });

    let target_plugin_label = req
        .target_plugin_label
        .clone()
        .or_else(|| names.plugin_name_for(plugin_index).map(str::to_owned))
        .unwrap_or_else(|| format!("plugin#{plugin_index}"));

    let preset = build_preset(
        &req.name,
        req.description.clone(),
        target_plugin_label,
        plugin_index,
        names,
        &snapshot,
    );

    let path = output_path(&req.root, req.category.as_deref(), &req.name);
    preset.save(&path)?;

    Ok(CaptureReport {
        written_to: path,
        preset,
        plugin_index,
        feed_was_active: snapshot.seen,
    })
}

/// Lightweight clone of the live feed state — same shape as
/// [`super::render::FeedSnapshot`] but pub(crate) so it doesn't need
/// to be re-typed by callers and the renderer can stay private.
struct Snapshot {
    params: std::collections::HashMap<(u32, u32), f32>,
    seen: bool,
}

fn read_feed(feed: &Arc<RwLock<CarlaFeed>>) -> Snapshot {
    let g = feed.read().unwrap_or_else(|p| p.into_inner());
    Snapshot {
        params: g.params.clone(),
        seen: g.seen_first_message,
    }
}

fn build_preset(
    name: &str,
    description: Option<String>,
    target_plugin: String,
    plugin_index: u32,
    names: &NameMap,
    snapshot: &Snapshot,
) -> Preset {
    let mut params: Vec<PresetParam> = snapshot
        .params
        .iter()
        .filter(|((p, _), _)| *p == plugin_index)
        .map(|((_, idx), value)| {
            // Prefer the friendly name when the project map knows it
            // so the preset file reads naturally. Falls back to the
            // numeric index when no map is loaded.
            let name_ref = names
                .param_name_for(plugin_index, *idx)
                .map(|n| ParamRef::Name(n.to_owned()))
                .unwrap_or(ParamRef::Index(*idx));
            PresetParam {
                name: name_ref,
                value: *value,
            }
        })
        .collect();
    params.sort_by(|a, b| {
        // Numeric ordering for index variants, alphabetical for names —
        // keeps the resulting TOML scannable in either case.
        let key = |p: &PresetParam| match &p.name {
            ParamRef::Index(i) => (0u8, *i as i64, String::new()),
            ParamRef::Name(n) => (1u8, 0i64, n.clone()),
        };
        key(a).cmp(&key(b))
    });
    Preset {
        name: Some(name.to_string()),
        description,
        target_plugin,
        params,
        files: Vec::new(),
        chunk: None,
    }
}

fn output_path(root: &Path, category: Option<&str>, name: &str) -> PathBuf {
    let dir = match category {
        Some(c) => root.join(c),
        None => root.to_path_buf(),
    };
    dir.join(format!("{}.preset.toml", sanitize_filename(name)))
}

/// Replace path-hostile characters with `_` so a preset name that
/// contains slashes or shell metacharacters still produces a sane
/// filename.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carla::carxp::{CarlaProject, ProjectParam, ProjectPlugin};

    fn fixture_map() -> NameMap {
        NameMap::from_project(&CarlaProject {
            plugins: vec![ProjectPlugin {
                slot: 0,
                name: "GxTuner".into(),
                plugin_type: Some("LV2".into()),
                uri: None,
                label: None,
                params: vec![
                    ProjectParam {
                        index: 0,
                        name: "FREQ".into(),
                        symbol: Some("FREQ".into()),
                    },
                    ProjectParam {
                        index: 5,
                        name: "THRESHOLD".into(),
                        symbol: Some("THRESHOLD".into()),
                    },
                ],
            }],
        })
    }

    #[test]
    fn build_preset_filters_by_plugin_index_and_uses_friendly_param_names() {
        let map = fixture_map();
        let snap = Snapshot {
            params: [((0, 0), 440.0), ((0, 5), -20.0), ((1, 0), 99.0)]
                .into_iter()
                .collect(),
            seen: true,
        };
        let preset = build_preset("Test", None, "GxTuner".into(), 0, &map, &snap);
        assert_eq!(preset.params.len(), 2, "plugin 1 should be filtered out");
        let names: Vec<&str> = preset
            .params
            .iter()
            .map(|p| match &p.name {
                ParamRef::Name(n) => n.as_str(),
                _ => panic!("expected named ref"),
            })
            .collect();
        assert!(names.contains(&"FREQ"));
        assert!(names.contains(&"THRESHOLD"));
    }

    #[test]
    fn build_preset_falls_back_to_indexed_refs_when_no_map_entry() {
        let map = NameMap::empty();
        let snap = Snapshot {
            params: [((3, 1), 0.5), ((3, 0), 0.25)].into_iter().collect(),
            seen: true,
        };
        let preset = build_preset("X", None, "Plug".into(), 3, &map, &snap);
        assert_eq!(preset.params.len(), 2);
        // Numeric ordering: 0 then 1.
        assert_eq!(preset.params[0].name, ParamRef::Index(0));
        assert_eq!(preset.params[1].name, ParamRef::Index(1));
    }

    #[test]
    fn output_path_drops_into_category_subdir_when_specified() {
        let root = PathBuf::from("/tmp/lib");
        let p = output_path(&root, Some("cabs"), "Vintage Marshall");
        assert_eq!(
            p,
            PathBuf::from("/tmp/lib/cabs/Vintage Marshall.preset.toml")
        );

        let p2 = output_path(&root, None, "rootlevel");
        assert_eq!(p2, PathBuf::from("/tmp/lib/rootlevel.preset.toml"));
    }

    #[test]
    fn output_path_sanitizes_path_hostile_characters() {
        let p = output_path(&PathBuf::from("/x"), None, "weird/name?with*chars");
        assert_eq!(
            p,
            PathBuf::from("/x/weird_name_with_chars.preset.toml"),
            "slashes and shell metacharacters become underscores"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_preset_writes_a_file_with_the_pushed_values() {
        // Stand up a fake Carla: bind a UDP socket, wait for /register,
        // push two /Carla/param packets back, then run capture_preset
        // in the same runtime.
        let std_carla = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        std_carla.set_nonblocking(true).unwrap();
        let target = std_carla.local_addr().unwrap();
        let carla_async = tokio::net::UdpSocket::from_std(std_carla).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let req = CaptureRequest {
            target,
            plugin: PluginRef::Index(0),
            name: "Live Capture".into(),
            description: Some("grabbed mid-session".into()),
            category: Some("cabs".into()),
            root: dir.path().to_path_buf(),
            // Long enough for the runtime to round-trip /register and a
            // few /Carla/param packets without flaking under load.
            capture_window: Duration::from_millis(800),
            target_plugin_label: Some("GxTuner".into()),
        };

        // Push the response packets *after* the capture_preset call
        // begins listening — easier to coordinate via a spawned task.
        let push_task = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (n, _) = tokio::time::timeout(
                Duration::from_millis(2_000),
                carla_async.recv_from(&mut buf),
            )
            .await
            .expect("/register should arrive")
            .unwrap();
            let (_, pkt) = rosc::decoder::decode_udp(&buf[..n]).unwrap();
            let rosc::OscPacket::Message(msg) = pkt else {
                panic!("expected /register");
            };
            assert_eq!(msg.addr, "/register");
            let url = match &msg.args[0] {
                rosc::OscType::String(s) => s.clone(),
                _ => panic!("/register URL"),
            };
            let host_port = url
                .trim_start_matches("osc.udp://")
                .split('/')
                .next()
                .unwrap();
            let listener_addr: SocketAddr = host_port.parse().unwrap();
            for (idx, value) in [(0u32, 440.0f32), (5u32, -20.0f32)] {
                let pkt = rosc::OscPacket::Message(rosc::OscMessage {
                    addr: "/Carla/param".into(),
                    args: vec![
                        rosc::OscType::Int(0),
                        rosc::OscType::Int(idx as i32),
                        rosc::OscType::Float(value),
                    ],
                });
                carla_async
                    .send_to(&rosc::encoder::encode(&pkt).unwrap(), listener_addr)
                    .await
                    .unwrap();
            }
        });

        // capture_preset blocks via its own runtime, so spawn it on a
        // blocking thread to avoid nesting tokio runtimes.
        let names = fixture_map();
        let req_clone = req.clone();
        let report = tokio::task::spawn_blocking(move || capture_preset(req_clone, &names))
            .await
            .unwrap()
            .expect("capture should succeed");

        push_task.await.unwrap();

        assert!(report.feed_was_active, "listener should have seen Carla");
        assert_eq!(report.plugin_index, 0);
        assert_eq!(report.preset.params.len(), 2);
        assert!(report.written_to.exists());
        let body = std::fs::read_to_string(&report.written_to).unwrap();
        assert!(body.contains("FREQ"));
        assert!(body.contains("THRESHOLD"));
        assert!(body.contains("440"));
    }

    #[test]
    fn capture_preset_errors_when_plugin_name_unresolvable() {
        let dir = tempfile::tempdir().unwrap();
        let req = CaptureRequest {
            target: "127.0.0.1:1".parse().unwrap(),
            plugin: PluginRef::Name("ImaginaryPlugin".into()),
            name: "X".into(),
            description: None,
            category: None,
            root: dir.path().to_path_buf(),
            capture_window: Duration::from_millis(50),
            target_plugin_label: None,
        };
        let names = NameMap::empty();
        let err = capture_preset(req, &names).unwrap_err();
        assert!(err.to_string().contains("not in name map"));
    }
}
