//! deck.page_goto action — request page switch via bus.
//!
//! Args:
//!   page : string (required)

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct DeckPageGoto {
    page: String,
}

impl BuildFromArgs for DeckPageGoto {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let page = args
            .get("page")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("deck.page_goto requires page".into()))?
            .to_string();
        Ok(Self { page })
    }
}

impl Action for DeckPageGoto {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.bus.publish(
            "deck.page_switch_request",
            serde_json::json!({"page": &self.page}),
        );
        cx.tile.flash(100);
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Nav
    }
}
