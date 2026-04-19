//! Top-level CLI. Parses arguments + runs the deck.

use crate::app::DeckApp;
use crate::config::DeckPaths;
use crate::render::{emit_page_appear, on_frame};
use crate::Result;
use clap::{Parser, Subcommand};
use juballer_core::{App, PresentMode};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "juballer-deck",
    version,
    about = "Stream-Deck-style app on GAMO2 FB9 via juballer-core"
)]
pub struct Cli {
    /// Path to config dir (default: ~/.config/juballer/deck)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Override active profile for this run.
    #[arg(long)]
    pub profile: Option<String>,

    /// Override monitor description for this run.
    #[arg(long)]
    pub monitor: Option<String>,

    /// Enable debug overlay from juballer-core.
    #[arg(long)]
    pub debug: bool,

    /// Render one frame and exit (headless smoke).
    #[arg(long)]
    pub once: bool,

    #[command(subcommand)]
    pub cmd: Option<SubCmd>,
}

#[derive(Subcommand, Debug)]
pub enum SubCmd {
    /// Validate config and exit.
    Check,
    /// List profiles in the config dir.
    ProfileList,
    /// Run the interactive calibration overlay (geometry + keymap). Writes the updated
    /// profile on Enter and exits normally; press Escape to cancel.
    Calibrate {
        /// Skip geometry phase and re-learn the keymap only.
        #[arg(long)]
        keymap_only: bool,
    },
    /// Rhythm-game mode: play a memon v1.0.0 chart, or pick one from a directory.
    /// If `CHART` is a file, plays it directly. If it's a directory, scans for
    /// `*.memon` files and shows a 4×4 chart-select grid; pressing a cell
    /// re-execs `play <file>` for that chart. If `CHART` is omitted, falls
    /// back to `rhythm.charts_dir` from the deck config.
    Play {
        /// Path to the `.memon` (JSON) chart file OR a directory of them. When
        /// omitted, the configured `rhythm.charts_dir` from deck.toml is used.
        #[arg(value_name = "CHART")]
        chart: Option<PathBuf>,
        /// Difficulty key to load (must be present under `data` in the chart).
        #[arg(long, default_value = "BSC")]
        difficulty: String,
        /// Audio offset in ms. Positive = audio lags input → we subtract from music_time.
        #[arg(long, default_value_t = 0)]
        audio_offset_ms: i32,
        /// Silence per-grade hit sound effects. Does not affect the song.
        #[arg(long, default_value_t = false)]
        mute_sfx: bool,
        /// Master volume for hit SFX in 0.0..=1.0. Overrides the bank
        /// default (~0.35). Independent of `--mute-sfx`.
        #[arg(long)]
        sfx_volume: Option<f32>,
    },
    /// Audio-offset calibration: plays a short 120 BPM metronome, the player
    /// taps cell (1,1) on every click, and the recommended `--audio-offset-ms`
    /// for subsequent runs is printed at the end.
    CalibrateAudio {
        /// Starting offset in ms. Pass your current best guess; the output
        /// will be a delta from this. Default 0.
        #[arg(long, default_value_t = 0)]
        audio_offset_ms: i32,
    },
    /// In-app rhythm settings editor. Fullscreen 4×4 grid; each row tunes one
    /// persistent field in `[rhythm]` (audio offset, volume, SFX). Cell (3,3)
    /// exits and writes back to `deck.toml`.
    Settings,
    /// Gameplay-mods editor. Fullscreen toggle grid — currently one flag
    /// (no-fail) persisted under `[rhythm.mods]` in `deck.toml`. Designed
    /// to absorb more flags without UI restructuring.
    Mods,
    /// Tutorial mode: short scripted rhythm session with narration overlays
    /// teaching the basics (tap timing, holds, exit gesture). Uses the same
    /// metronome audio as `calibrate-audio`.
    Tutorial {
        /// Audio offset in ms. Pass your calibrated value so the narration
        /// + note approach are perceived in sync.
        #[arg(long, default_value_t = 0)]
        audio_offset_ms: i32,
    },
}

pub fn run(cli: Cli) -> Result<()> {
    let cfg_root = cli.config.unwrap_or_else(crate::config::default_config_dir);
    let paths = DeckPaths::from_root(cfg_root.clone());

    // Pre-parse deck.toml just for the [log] section so file logging
    // is configured before anything noisy runs. Falls back to
    // LogConfig::default() if the file is missing or malformed — we
    // still want logs even when the config is in a bad state.
    let log_cfg: crate::config::LogConfig = std::fs::read_to_string(&paths.deck_toml)
        .ok()
        .and_then(|s| toml::from_str::<crate::config::DeckConfig>(&s).ok())
        .map(|d| d.log)
        .unwrap_or_default();
    let _log_handles = crate::logging::init(&cfg_root, &log_cfg);
    let log_dir_for_watch = _log_handles.dir.clone();

    let calibrate_mode: Option<bool> = match cli.cmd {
        Some(SubCmd::Check) => {
            let tree = crate::config::ConfigTree::load(&paths)?;
            println!(
                "deck OK. profiles: {:?}",
                tree.profiles.keys().collect::<Vec<_>>()
            );
            return Ok(());
        }
        Some(SubCmd::ProfileList) => {
            let tree = crate::config::ConfigTree::load(&paths)?;
            for (name, p) in &tree.profiles {
                println!("{}  {}", name, p.meta.description);
            }
            return Ok(());
        }
        Some(SubCmd::Calibrate { keymap_only }) => Some(keymap_only),
        Some(SubCmd::Play {
            chart,
            difficulty,
            audio_offset_ms,
            mute_sfx,
            sfx_volume,
        }) => {
            // Rhythm mode owns its own winit event loop + window; it bypasses the
            // DeckApp entirely. If `chart` is a directory, launch the picker —
            // which re-execs us with `play <file>` once the user taps a cell.
            let tree = crate::config::ConfigTree::load(&paths)?;
            let rhythm_cfg = tree.deck.rhythm.clone();
            let chart = match chart {
                Some(p) => p,
                None => match rhythm_cfg.charts_dir.clone() {
                    Some(p) => p,
                    // Implicit fallback: `~/.config/juballer/rhythm/charts/`
                    // so a fresh install with no `charts_dir` in deck.toml
                    // still works once the user drops charts into the
                    // conventional location.
                    None => {
                        let home =
                            std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
                                crate::Error::Config(
                                    "no chart specified, rhythm.charts_dir unset, $HOME unset"
                                        .into(),
                                )
                            })?;
                        home.join(".config/juballer/rhythm/charts")
                    }
                },
            };
            let opts = crate::rhythm::PlayOpts {
                no_fail: rhythm_cfg.mods.no_fail,
                lead_in_ms: rhythm_cfg.lead_in_ms,
                asset_dir: rhythm_cfg.asset_dir.clone(),
                backgrounds: rhythm_cfg.backgrounds.clone(),
                background_index: rhythm_cfg.background_index,
            };
            if chart.is_dir() {
                return crate::rhythm::pick(
                    &chart,
                    &difficulty,
                    audio_offset_ms,
                    mute_sfx,
                    sfx_volume,
                    opts.backgrounds.clone(),
                    opts.background_index,
                );
            }
            return crate::rhythm::play_with_opts(
                &chart,
                &difficulty,
                audio_offset_ms,
                mute_sfx,
                sfx_volume,
                opts,
            );
        }
        Some(SubCmd::CalibrateAudio { audio_offset_ms }) => {
            return crate::rhythm::calibrate_audio(audio_offset_ms);
        }
        Some(SubCmd::Settings) => {
            return crate::rhythm::settings(&paths);
        }
        Some(SubCmd::Mods) => {
            return crate::rhythm::mods_ui::run(&paths);
        }
        Some(SubCmd::Tutorial { audio_offset_ms }) => {
            return crate::rhythm::run_tutorial(audio_offset_ms);
        }
        None => None,
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let mut deck = DeckApp::bootstrap(paths, rt.handle().clone())?;
    if let Some(profile) = cli.profile {
        deck.config.deck.active_profile = profile;
        deck.bind_active_page()?;
    }

    wire_plugin_host(&mut deck, rt.handle());
    let editor_wired = wire_editor_server(&mut deck, rt.handle());

    let monitor_desc = cli
        .monitor
        .or_else(|| deck.config.deck.render.monitor_desc.clone());

    let present_mode = match deck.config.deck.render.present_mode.as_deref() {
        Some("immediate") => PresentMode::Immediate,
        Some("mailbox") => PresentMode::Mailbox,
        _ => PresentMode::Fifo,
    };

    let mut builder = App::builder()
        .title("juballer-deck")
        .present_mode(present_mode)
        .bg_color(deck.bg_color())
        .controller_vid_pid(0x1973, 0x0011); // GAMO2 FB9 (TODO: make configurable)
    if let Some(m) = &monitor_desc {
        builder = builder.on_monitor(m.clone());
    }
    let mut app = builder.build()?;

    if cli.debug {
        app.set_debug(true);
    }

    // `juballer-deck calibrate` forces the calibration overlay on startup.
    // The overlay writes the updated profile atomically on Enter; Escape cancels.
    match calibrate_mode {
        Some(true) => {
            app.run_keymap_auto_learn()?;
        }
        Some(false) => {
            app.run_calibration()?;
        }
        None => {}
    }

    // Initial top layout: bootstrap queued one for the active page; install it on the App
    // pre-run so the first frame already has it solved (avoids a one-frame empty top region).
    if let Some(Some(root)) = deck.pending_top_layout.as_ref() {
        app.set_top_layout(root.clone());
        tracing::info!(
            "top layout applied: {} panes",
            deck.last_applied_top_pane_names.len()
        );
    }

    // Config watcher: signals land on a stdlib channel; a helper thread sets a flag
    // the render loop checks each frame.
    // Ignore log writes — the rolling appender lives under the config root
    // by default, so without this filter every log line refires the reload
    // signal and clears+rebuilds widgets at log cadence.
    let (_watcher, reload_rx) = crate::config::watch(
        &deck.paths.root.clone(),
        std::time::Duration::from_millis(300),
        vec![log_dir_for_watch],
    )?;
    let reload_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let flag = reload_flag.clone();
        std::thread::Builder::new()
            .name("juballer-deck-reload-signal".into())
            .spawn(move || {
                for _sig in reload_rx.iter() {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            })
            .expect("reload signal thread");
    }

    emit_page_appear(&mut deck);
    let mut frame_count = 0u64;
    let once = cli.once;

    app.run(move |frame, events| {
        if reload_flag.swap(false, std::sync::atomic::Ordering::Relaxed) {
            match crate::config::ConfigTree::load(&deck.paths) {
                Ok(new_config) => {
                    let new_active = new_config.deck.active_profile.clone();
                    deck.config = new_config.clone();
                    if let Err(e) = deck.bind_active_page() {
                        tracing::warn!("reload: rebind failed: {e}");
                    } else {
                        tracing::info!("reload: config applied");
                        crate::render::emit_page_appear(&mut deck);
                    }
                    // Refresh the editor's snapshot + push a profile_reloaded over WS.
                    if let Some((tx, cfg_arc)) = editor_wired.as_ref() {
                        if let Ok(mut g) = cfg_arc.lock() {
                            *g = new_config;
                        }
                        let _ = tx.send(crate::editor::server::EditorEvent::ProfileReloaded {
                            profile: new_active,
                        });
                    }
                }
                Err(e) => tracing::warn!("reload: config load failed: {e}"),
            }
        }
        on_frame(&mut deck, frame, events);
        frame_count += 1;
        if once && frame_count >= 1 {
            std::process::exit(0);
        }
    })?;
    Ok(())
}

fn wire_plugin_host(deck: &mut DeckApp, rt: &tokio::runtime::Handle) {
    let plugins_dir = deck.paths.plugins_dir.clone();
    if !plugins_dir.exists() {
        tracing::info!(
            "plugin host: no plugins dir at {:?} (skipping)",
            plugins_dir
        );
        return;
    }
    let mut host = crate::plugin::host::PluginHost::new(plugins_dir);
    let manifests = match host.discover() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("plugin host: discover failed: {}", e);
            return;
        }
    };
    if manifests.is_empty() {
        tracing::info!("plugin host: no plugin manifests found");
        deck.plugin_host = Some(std::sync::Arc::new(tokio::sync::Mutex::new(host)));
        return;
    }
    for m in &manifests {
        tracing::info!(
            "plugin host: discovered '{}' v{} ({} actions, {} widgets)",
            m.name,
            m.version,
            m.actions.len(),
            m.widgets.len()
        );
    }
    let rt_for_spawn = rt.clone();
    let view_trees = deck.view_trees.clone();
    let named_tiles = deck.named_tile_overrides.clone();
    let spawned = rt.block_on(async move {
        if let Err(e) = host.spawn_all(&rt_for_spawn, view_trees, named_tiles).await {
            tracing::warn!("plugin host: spawn_all failed: {}", e);
        }
        host
    });
    for (plugin_name, conn) in &spawned.plugins {
        crate::action::builtin::plugin_proxy_action::register_plugin_sender(
            plugin_name.clone(),
            conn.send.clone(),
        );
        for action_name in &conn.manifest.actions {
            let pn = plugin_name.clone();
            let an = action_name.clone();
            let static_name: &'static str = Box::leak(action_name.clone().into_boxed_str());
            deck.actions.register_factory(
                static_name,
                Box::new(move |args: &toml::Table| {
                    let json_args = serde_json::to_value(args).unwrap_or(serde_json::json!({}));
                    Ok(Box::new(
                        crate::action::builtin::plugin_proxy_action::PluginProxyAction::new(
                            pn.clone(),
                            an.clone(),
                            json_args,
                        ),
                    ))
                }),
            );
        }
    }
    deck.plugin_host = Some(std::sync::Arc::new(tokio::sync::Mutex::new(spawned)));
}

/// Wire the editor server. Returns the editor's broadcast sender + the shared in-memory
/// `ConfigTree` mutex so the run loop can publish reload events and refresh the snapshot.
/// Returns `None` if the editor is disabled.
#[allow(clippy::type_complexity)]
fn wire_editor_server(
    deck: &mut DeckApp,
    rt: &tokio::runtime::Handle,
) -> Option<(
    tokio::sync::broadcast::Sender<crate::editor::server::EditorEvent>,
    std::sync::Arc<std::sync::Mutex<crate::config::ConfigTree>>,
)> {
    let editor = &deck.config.deck.editor;
    if !editor.enabled {
        tracing::info!("editor: disabled by config");
        return None;
    }
    let bind: std::net::SocketAddr = match editor.bind.parse() {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("editor: invalid bind '{}': {}", editor.bind, e);
            return None;
        }
    };
    let action_names: Vec<String> = deck.actions.names().map(String::from).collect();
    let widget_names: Vec<String> = deck.widgets.names().map(String::from).collect();
    let action_schemas: std::collections::HashMap<String, serde_json::Value> = action_names
        .iter()
        .filter_map(|n| deck.actions.schema_for(n).map(|s| (n.clone(), s)))
        .collect();
    let widget_schemas: std::collections::HashMap<String, serde_json::Value> = widget_names
        .iter()
        .filter_map(|n| deck.widgets.schema_for(n).map(|s| (n.clone(), s)))
        .collect();
    let plugin_names: Vec<String> = deck
        .plugin_host
        .as_ref()
        .map(|h| h.blocking_lock().plugins.keys().cloned().collect())
        .unwrap_or_default();

    // Editor → WS broadcast channel. Capacity 64 is plenty (events are small + sparse).
    let (bus_tx, _) = tokio::sync::broadcast::channel(64);

    // Wire plugin status events into the editor bus. The plugin host emits
    // PluginStatusEvent on its own broadcast channel; we relay those through.
    if let Some(host) = deck.plugin_host.as_ref() {
        let (status_tx, mut status_rx) =
            tokio::sync::broadcast::channel::<crate::plugin::host::PluginStatusEvent>(64);
        host.blocking_lock().set_status_tx(status_tx);
        let bus_tx_for_status = bus_tx.clone();
        rt.spawn(async move {
            loop {
                match status_rx.recv().await {
                    Ok(ev) => {
                        let _ = bus_tx_for_status.send(
                            crate::editor::server::EditorEvent::PluginStatus {
                                name: ev.name,
                                status: ev.status.as_str().to_string(),
                            },
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    let auth_token = if editor.require_auth {
        // No token persistence yet — fall back to env JUBALLER_EDITOR_TOKEN if present;
        // otherwise generate one and log it (operator copies into the SPA).
        match std::env::var("JUBALLER_EDITOR_TOKEN") {
            Ok(t) if !t.is_empty() => Some(t),
            _ => {
                let t: String = (0..32)
                    .map(|_| {
                        let n = rand::random::<u8>() & 0x0f;
                        std::char::from_digit(u32::from(n), 16).unwrap()
                    })
                    .collect();
                tracing::info!("editor: require_auth=true, generated token: {}", t);
                Some(t)
            }
        }
    } else {
        None
    };

    let config = std::sync::Arc::new(std::sync::Mutex::new(deck.config.clone()));
    let state = std::sync::Arc::new(crate::editor::server::EditorState {
        config: config.clone(),
        paths: deck.paths.clone(),
        auth_token,
        plugin_host: deck.plugin_host.clone(),
        rt: rt.clone(),
        deck_bus: deck.bus.clone(),
        action_names,
        widget_names,
        plugin_names,
        action_schemas,
        widget_schemas,
        bus_tx: bus_tx.clone(),
    });
    deck.editor_event_tx = Some(bus_tx.clone());
    let server = crate::editor::server::EditorServer::new(bind, state);
    if let Err(e) = server.spawn(rt) {
        tracing::warn!("editor: spawn failed: {}", e);
    }
    Some((bus_tx, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_basic_flags() {
        let c =
            Cli::try_parse_from(["juballer-deck", "--config", "/x", "--debug", "--once"]).unwrap();
        assert_eq!(c.config.unwrap(), PathBuf::from("/x"));
        assert!(c.debug);
        assert!(c.once);
    }

    #[test]
    fn parses_check_subcmd() {
        let c = Cli::try_parse_from(["juballer-deck", "check"]).unwrap();
        assert!(matches!(c.cmd, Some(SubCmd::Check)));
    }

    #[test]
    fn parses_settings_subcmd() {
        let c = Cli::try_parse_from(["juballer-deck", "settings"]).unwrap();
        assert!(matches!(c.cmd, Some(SubCmd::Settings)));
    }

    #[test]
    fn parses_play_with_no_chart() {
        // `play` with no CHART should parse (chart = None); fallback to config
        // happens at run time, not parse time.
        let c = Cli::try_parse_from(["juballer-deck", "play"]).unwrap();
        match c.cmd {
            Some(SubCmd::Play { chart, .. }) => assert!(chart.is_none()),
            other => panic!("expected Play, got {:?}", other),
        }
    }

    #[test]
    fn parses_play_with_explicit_chart() {
        let c = Cli::try_parse_from(["juballer-deck", "play", "/tmp/x.memon"]).unwrap();
        match c.cmd {
            Some(SubCmd::Play { chart, .. }) => {
                assert_eq!(chart.as_deref(), Some(std::path::Path::new("/tmp/x.memon")));
            }
            other => panic!("expected Play, got {:?}", other),
        }
    }

    /// `tutorial` subcommand should parse and accept --audio-offset-ms.
    /// Guards the plumbing between CLI → rhythm::run_tutorial.
    #[test]
    fn parses_tutorial_subcmd() {
        let c = Cli::try_parse_from(["juballer-deck", "tutorial"]).unwrap();
        match c.cmd {
            Some(SubCmd::Tutorial { audio_offset_ms }) => assert_eq!(audio_offset_ms, 0),
            other => panic!("expected Tutorial, got {:?}", other),
        }
        let c =
            Cli::try_parse_from(["juballer-deck", "tutorial", "--audio-offset-ms", "42"]).unwrap();
        match c.cmd {
            Some(SubCmd::Tutorial { audio_offset_ms }) => assert_eq!(audio_offset_ms, 42),
            other => panic!("expected Tutorial, got {:?}", other),
        }
    }

    #[test]
    fn parses_play_sfx_volume_flag() {
        let cli = Cli::try_parse_from(["juballer-deck", "play", "/tmp/x.memon"]).unwrap();
        match cli.cmd {
            Some(SubCmd::Play { sfx_volume, .. }) => {
                assert!(sfx_volume.is_none(), "sfx_volume defaults to None");
            }
            _ => panic!("expected Play subcommand"),
        }
        let cli = Cli::try_parse_from([
            "juballer-deck",
            "play",
            "/tmp/x.memon",
            "--sfx-volume",
            "0.5",
        ])
        .unwrap();
        match cli.cmd {
            Some(SubCmd::Play { sfx_volume, .. }) => {
                assert_eq!(sfx_volume, Some(0.5));
            }
            _ => panic!("expected Play subcommand"),
        }
    }

    /// `--mute-sfx` must parse on `play` and default to false when omitted.
    /// Guards against silently renaming/removing the flag — the sfx bank
    /// relies on this plumbing.
    #[test]
    fn parses_play_mute_sfx_flag() {
        let c = Cli::try_parse_from(["juballer-deck", "play", "/tmp/x.memon"]).unwrap();
        match c.cmd {
            Some(SubCmd::Play { mute_sfx, .. }) => {
                assert!(!mute_sfx, "mute_sfx should default to false");
            }
            other => panic!("expected Play, got {:?}", other),
        }
        let c =
            Cli::try_parse_from(["juballer-deck", "play", "/tmp/x.memon", "--mute-sfx"]).unwrap();
        match c.cmd {
            Some(SubCmd::Play { mute_sfx, .. }) => {
                assert!(mute_sfx, "--mute-sfx should flip to true");
            }
            other => panic!("expected Play, got {:?}", other),
        }
    }
}
