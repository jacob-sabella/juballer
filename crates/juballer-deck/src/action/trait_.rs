//! The Action trait + ActionCx — the contract every action (built-in or plugin) follows.

use crate::bus::EventBus;
use crate::state::StateStore;
use crate::tile::TileHandle;
use crate::Result;
use indexmap::IndexMap;

/// Per-frame context passed to action callbacks.
pub struct ActionCx<'a> {
    pub cell: (u8, u8),
    pub binding_id: &'a str,
    pub tile: TileHandle<'a>,
    pub env: &'a IndexMap<String, String>,
    pub bus: &'a EventBus,
    pub state: &'a mut StateStore,
    pub rt: &'a tokio::runtime::Handle,
}

/// Classification of an action's intent — drives tile rendering so the user can
/// distinguish navigation affordances from side-effecting buttons and stateful
/// toggles at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Performs a real-world side effect (shell, HTTP, keypress, media, …).
    Action,
    /// Navigation — moves the user to another page / profile.
    Nav,
    /// Toggle — binary or cycling state that the tile often reflects in its accent color.
    Toggle,
}

/// Long-lived object bound to one (page, row, col). Full lifecycle per spec.
pub trait Action: Send + 'static {
    fn on_will_appear(&mut self, cx: &mut ActionCx<'_>) {
        let _ = cx;
    }
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let _ = cx;
    }
    fn on_up(&mut self, cx: &mut ActionCx<'_>) {
        let _ = cx;
    }
    fn on_will_disappear(&mut self, cx: &mut ActionCx<'_>) {
        let _ = cx;
    }

    /// Classifies this action for rendering purposes. Default is `Action`.
    /// Nav/Toggle variants get a distinct visual treatment in the tile layer.
    fn kind(&self) -> ActionKind {
        ActionKind::Action
    }
}

/// Build-from-args trait — used by the registry to instantiate actions from TOML.
pub trait BuildFromArgs: Sized {
    fn from_args(args: &toml::Table) -> Result<Self>;
}
