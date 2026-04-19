//! deck.page_back action — pop page history (handled by deck via bus).

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct DeckPageBack;

impl BuildFromArgs for DeckPageBack {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for DeckPageBack {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.bus
            .publish("deck.page_back_request", serde_json::json!({}));
        cx.tile.flash(100);
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Nav
    }
}
