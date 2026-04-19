//! deck.scroll_down action — request the deck to scroll the visible window down by 1 row.

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct DeckScrollDown;

impl BuildFromArgs for DeckScrollDown {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for DeckScrollDown {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.bus
            .publish("deck.scroll_request", serde_json::json!({"dr": 1, "dc": 0}));
        cx.tile.flash(100);
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Nav
    }
}
