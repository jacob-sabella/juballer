//! juballer-deck — Stream-Deck-style application built on juballer-core.
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod action;
pub mod app;
pub mod bus;
pub mod cli;
pub mod config;
pub mod editor;
mod error;
pub mod icon_loader;
pub mod layout_convert;
pub mod logging;
pub mod plugin;
pub mod render;
pub mod rhythm;
pub mod shader;
pub mod state;
pub mod theme;
pub mod tile;
pub mod video;
pub mod widget;
pub use action::{Action, ActionCx, ActionRegistry, BuildFromArgs};
pub use app::DeckApp;
pub use bus::{Event, EventBus};
pub use cli::{Cli, SubCmd};
pub use error::{Error, Result};
pub use state::StateStore;
pub use theme::Theme;
pub use tile::{IconRef, TileHandle, TileState};
pub use widget::{Widget, WidgetCx, WidgetRegistry};
