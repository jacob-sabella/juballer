//! Carla integration mode — use the 4×4 grid as a control surface for
//! a running [Carla](https://kx.studio/Applications:Carla) plugin host.
//!
//! The grid sends OSC messages to Carla's built-in OSC server (default
//! `127.0.0.1:22752`) to bump / toggle / set plugin parameters. Cells
//! are bound via TOML configuration files that live in
//! `~/.config/juballer/carla/configs/<name>.toml`. Each configuration
//! defines one or more "pages" of cell bindings; juballer's
//! `Paginator` cycles between sub-pages while the bottom row of the
//! grid is reserved for navigation:
//!
//! ```text
//! cell (3,0) = PAGE-PREV   cell (3,1) = PAGE-NEXT
//! cell (3,2) = CONFIGS     cell (3,3) = EXIT
//! ```
//!
//! ## Phasing
//!
//! Phase 1 (this revision) implements **input cells only** — `bump-up`,
//! `bump-down`, `toggle`, `momentary`, `set`, `carousel-next`,
//! `carousel-prev`. Display cells (`tuner`, `meter`, `value`, `text`,
//! `active-preset-name`) and preset cells (`load-preset`,
//! `open-preset-picker`) are parsed and validated so configurations
//! written today survive the upgrade to Phase 2 / 3, but their
//! behavior is currently a no-op (`load-preset` logs an info line so
//! the operator can confirm the binding fired).

pub mod config;
pub mod dispatch;
pub mod listener;
pub mod osc;
pub mod picker;
pub mod render;
pub mod state;

use crate::carla::dispatch::{CellEvent, Outcome};
use crate::Result;
use juballer_core::input::Event;
use juballer_core::{App, Color, PresentMode};
use juballer_egui::EguiOverlay;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;

/// Run Carla mode against the configuration at `path`. Opens its own
/// fullscreen winit window and never returns until the user exits via
/// the EXIT cell, Escape, or window close. The deck process is then
/// re-execed back into the deck via `juballer_core::process::exit` so
/// the operator lands where they came from.
pub fn run(path: &Path) -> Result<()> {
    let cfg = config::Configuration::load(path)?;
    let target = resolve_target(&cfg.carla)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| crate::Error::Config(format!("tokio runtime: {e}")))?;
    let client = osc::CarlaClient::spawn(rt.handle(), target)?;
    let live_listener = listener::spawn(rt.handle(), target).ok();
    let live_feed = live_listener.as_ref().map(listener::CarlaListener::feed);

    let mut state = state::CarlaState::new(cfg);
    let mut press_starts: HashMap<(u8, u8), Instant> = HashMap::new();
    let configs_dir = config::default_configs_dir();
    let mut picker_state: Option<picker::PickerState> = None;

    let mut app = App::builder()
        .title("juballer — carla")
        .present_mode(PresentMode::Fifo)
        .bg_color(Color::BLACK)
        .controller_vid_pid(0x1973, 0x0011)
        .build()?;
    app.set_debug(false);

    let mut overlay = EguiOverlay::new();
    let exit_client = client.clone();
    let exit_listener = live_listener.clone();

    app.run(move |frame, events| {
        state.tick();
        if let Some(p) = picker_state.as_mut() {
            p.tick();
            render::paint_picker(frame, p);
            render::draw_picker_overlay(frame, &mut overlay, p);
        } else {
            render::paint_backgrounds(frame, &state);
            render::draw_overlay(frame, &mut overlay, &state, live_feed.as_ref());
        }

        for ev in events {
            match ev {
                Event::KeyDown { row, col, .. } => {
                    if picker_state.is_some() {
                        // Picker is press-on-release; ignore key-down so a
                        // bumped finger doesn't activate the cell underneath.
                        continue;
                    }
                    on_key_down(*row, *col, &mut press_starts, &mut state, &client);
                }
                Event::KeyUp { row, col, .. } => {
                    if let Some(p) = picker_state.as_mut() {
                        match picker::classify_press(p, *row, *col) {
                            picker::PickerAction::PagePrev => {
                                p.prev_page();
                            }
                            picker::PickerAction::PageNext => {
                                p.next_page();
                            }
                            picker::PickerAction::Back => {
                                picker_state = None;
                            }
                            picker::PickerAction::Exit => {
                                exit_client.shutdown();
                                if let Some(l) = exit_listener.as_ref() {
                                    l.shutdown();
                                }
                                juballer_core::process::exit(0);
                            }
                            picker::PickerAction::Activate(path) => {
                                match config::Configuration::load(&path) {
                                    Ok(new_cfg) => {
                                        state = state::CarlaState::new(new_cfg);
                                        press_starts.clear();
                                        picker_state = None;
                                        tracing::info!(
                                            target: "juballer::carla",
                                            "activated {}",
                                            path.display()
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            target: "juballer::carla",
                                            "failed to load {}: {e}",
                                            path.display()
                                        );
                                    }
                                }
                            }
                            picker::PickerAction::None => {}
                        }
                        continue;
                    }
                    if *row == render::NAV_ROW && *col == render::NAV_PICKER_COL {
                        // Clear any stale press state before swapping screens
                        // so a bottom-row finger doesn't carry into picker
                        // event handling on the next frame.
                        press_starts.clear();
                        let entries = picker::scan(&configs_dir);
                        if entries.is_empty() {
                            tracing::info!(
                                target: "juballer::carla",
                                "no configs found under {}",
                                configs_dir.display()
                            );
                        } else {
                            picker_state = Some(picker::PickerState::new(entries));
                        }
                        continue;
                    }
                    on_key_up(
                        *row,
                        *col,
                        &mut press_starts,
                        &mut state,
                        &client,
                        live_listener.as_ref(),
                    );
                }
                Event::Quit => {
                    exit_client.shutdown();
                    if let Some(l) = exit_listener.as_ref() {
                        l.shutdown();
                    }
                    juballer_core::process::exit(0);
                }
                Event::Unmapped { key, .. } if key.0 == "NAMED_Escape" => {
                    if picker_state.is_some() {
                        // Escape inside the picker drops back to the
                        // active grid rather than exiting the mode.
                        picker_state = None;
                    } else {
                        exit_client.shutdown();
                        if let Some(l) = exit_listener.as_ref() {
                            l.shutdown();
                        }
                        juballer_core::process::exit(0);
                    }
                }
                _ => {}
            }
        }
    })?;
    Ok(())
}

/// Parse the host:port out of the user's `[carla]` block. Surfaces a
/// readable error if the host can't be resolved synchronously, since
/// silently falling back would mask a config typo.
fn resolve_target(target: &config::CarlaTarget) -> Result<SocketAddr> {
    use std::net::ToSocketAddrs;
    let key = format!("{}:{}", target.host, target.port);
    let mut iter = key
        .to_socket_addrs()
        .map_err(|e| crate::Error::Config(format!("carla target {key}: {e}")))?;
    iter.next()
        .ok_or_else(|| crate::Error::Config(format!("carla target {key}: no addresses resolved")))
}

fn on_key_down(
    row: u8,
    col: u8,
    press_starts: &mut HashMap<(u8, u8), Instant>,
    state: &mut state::CarlaState,
    client: &osc::CarlaClient,
) {
    press_starts.insert((row, col), Instant::now());
    if row == render::NAV_ROW {
        return; // nav keys handled on release
    }
    let Some(cell) = lookup_cell(state, row, col) else {
        return;
    };
    let outcomes = dispatch::dispatch(&cell, CellEvent::KeyDown, state.cache_mut());
    for outcome in outcomes {
        handle_outcome(outcome, client, state);
    }
}

fn on_key_up(
    row: u8,
    col: u8,
    press_starts: &mut HashMap<(u8, u8), Instant>,
    state: &mut state::CarlaState,
    client: &osc::CarlaClient,
    live: Option<&listener::CarlaListener>,
) {
    let held = press_starts
        .remove(&(row, col))
        .map(|t| t.elapsed())
        .unwrap_or_default();

    if row == render::NAV_ROW {
        handle_nav(col, state, client, live);
        return;
    }

    let Some(cell) = lookup_cell(state, row, col) else {
        return;
    };
    let outcomes = dispatch::dispatch(&cell, CellEvent::KeyUp { held }, state.cache_mut());
    for outcome in outcomes {
        handle_outcome(outcome, client, state);
    }
}

/// Bottom-row navigation handler for the active grid. The CONFIGS
/// press + the picker overlay live in [`run`] because they need to
/// mutate per-frame state outside `state`.
fn handle_nav(
    col: u8,
    state: &mut state::CarlaState,
    client: &osc::CarlaClient,
    live: Option<&listener::CarlaListener>,
) {
    match col {
        c if c == render::NAV_PREV_COL => {
            state.prev_page();
        }
        c if c == render::NAV_NEXT_COL => {
            state.next_page();
        }
        c if c == render::NAV_EXIT_COL => {
            client.shutdown();
            if let Some(l) = live {
                l.shutdown();
            }
            juballer_core::process::exit(0);
        }
        _ => {}
    }
}

/// Pull the active-page cell at `(row, col)` as an owned clone so the
/// caller can release the borrow on `state` before mutating its cache.
fn lookup_cell(state: &state::CarlaState, row: u8, col: u8) -> Option<config::Cell> {
    state
        .active_page()
        .and_then(|page| page.cells.iter().find(|c| c.row == row && c.col == col))
        .cloned()
}

fn handle_outcome(outcome: Outcome, client: &osc::CarlaClient, state: &mut state::CarlaState) {
    match outcome {
        Outcome::SetParameter {
            plugin,
            param,
            value,
        } => {
            client.set_parameter_value(&plugin, &param, value);
            state.set_last_touched(plugin, param, value);
        }
        Outcome::LoadPreset { plugin, preset } => {
            tracing::info!(
                target: "juballer::carla",
                "load-preset {preset:?} for plugin {plugin:?} — Phase 3 stub"
            );
        }
        Outcome::OpenPresetPicker { category } => {
            tracing::info!(
                target: "juballer::carla",
                "open-preset-picker category={category:?} — Phase 3 stub"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carla::config::CarlaTarget;

    #[test]
    fn resolve_target_accepts_loopback_default() {
        let t = CarlaTarget::default();
        let addr = resolve_target(&t).unwrap();
        assert_eq!(addr.port(), config::DEFAULT_CARLA_PORT);
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn resolve_target_surfaces_unresolvable_host_as_config_error() {
        let t = CarlaTarget {
            host: "no-such-host.invalid.juballer-test".into(),
            port: 22752,
        };
        let err = resolve_target(&t).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("carla target"),
            "expected diagnostic prefix, got: {msg}"
        );
    }
}
