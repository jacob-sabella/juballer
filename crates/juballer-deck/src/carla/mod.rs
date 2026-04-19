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
//! cell 12 = PAGE-PREV   cell 13 = PAGE-NEXT
//! cell 14 = CONFIG-PICKER  cell 15 = EXIT
//! ```
//!
//! ## Phasing
//!
//! The TOML schema in [`config`] covers three phases. Phase 1 (this
//! revision) implements **input cells only** — `bump-up`, `bump-down`,
//! `toggle`, `momentary`, `set`. Display cells (`tuner`, `meter`,
//! `value`, `text`, `active-preset-name`) and preset cells
//! (`load-preset`, `open-preset-picker`) are parsed and validated so
//! configurations written today survive the upgrade to Phase 2 / 3,
//! but their behavior is currently a no-op.

pub mod config;
