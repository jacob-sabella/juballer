//! deck.scroll_up action — request the deck to scroll the visible window up by 1 row.

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct DeckScrollUp;

impl BuildFromArgs for DeckScrollUp {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for DeckScrollUp {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.bus.publish(
            "deck.scroll_request",
            serde_json::json!({"dr": -1, "dc": 0}),
        );
        cx.tile.flash(100);
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Nav
    }
}
