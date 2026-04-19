//! deck.profile_switch action — request profile switch via bus.
//!
//! Args:
//!   profile : string (required)

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct DeckProfileSwitch {
    profile: String,
}

impl BuildFromArgs for DeckProfileSwitch {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let profile = args
            .get("profile")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("deck.profile_switch requires profile".into()))?
            .to_string();
        Ok(Self { profile })
    }
}

impl Action for DeckProfileSwitch {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.bus.publish(
            "deck.profile_switch_request",
            serde_json::json!({"profile": &self.profile}),
        );
        cx.tile.flash(100);
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Nav
    }
}
