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
pub mod osc;
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

    let mut state = state::CarlaState::new(cfg);
    let mut press_starts: HashMap<(u8, u8), Instant> = HashMap::new();

    let mut app = App::builder()
        .title("juballer — carla")
        .present_mode(PresentMode::Fifo)
        .bg_color(Color::BLACK)
        .controller_vid_pid(0x1973, 0x0011)
        .build()?;
    app.set_debug(false);

    let mut overlay = EguiOverlay::new();
    let exit_client = client.clone();

    app.run(move |frame, events| {
        state.tick();
        render::paint_backgrounds(frame, &state);
        render::draw_overlay(frame, &mut overlay, &state);

        for ev in events {
            match ev {
                Event::KeyDown { row, col, .. } => {
                    on_key_down(*row, *col, &mut press_starts, &mut state, &client);
                }
                Event::KeyUp { row, col, .. } => {
                    on_key_up(*row, *col, &mut press_starts, &mut state, &client);
                }
                Event::Quit => {
                    exit_client.shutdown();
                    juballer_core::process::exit(0);
                }
                Event::Unmapped { key, .. } if key.0 == "NAMED_Escape" => {
                    exit_client.shutdown();
                    juballer_core::process::exit(0);
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
) {
    let held = press_starts
        .remove(&(row, col))
        .map(|t| t.elapsed())
        .unwrap_or_default();

    if row == render::NAV_ROW {
        handle_nav(col, state, client);
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

/// Bottom-row navigation handler. Phase 1: prev / next walk the
/// paginator, picker logs a placeholder, exit re-execs into the deck.
fn handle_nav(col: u8, state: &mut state::CarlaState, client: &osc::CarlaClient) {
    match col {
        c if c == render::NAV_PREV_COL => {
            state.prev_page();
        }
        c if c == render::NAV_NEXT_COL => {
            state.next_page();
        }
        c if c == render::NAV_PICKER_COL => {
            tracing::info!(
                target: "juballer::carla",
                "config picker overlay not implemented in Phase 1"
            );
        }
        c if c == render::NAV_EXIT_COL => {
            client.shutdown();
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
