//! deck.scroll_right action — request the deck to scroll the visible window right by 1 column.

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct DeckScrollRight;

impl BuildFromArgs for DeckScrollRight {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for DeckScrollRight {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.bus
            .publish("deck.scroll_request", serde_json::json!({"dr": 0, "dc": 1}));
        cx.tile.flash(100);
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Nav
    }
}
