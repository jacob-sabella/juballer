//! deck.hold_cycle action — short press = step page list forward; long press = step back.
//!
//! Args:
//!   pages          : array of string (required) — page names to cycle through
//!   long_press_ms  : u64 (default 400) — threshold for "long press"

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::{Error, Result};
use std::time::Instant;

#[derive(Debug)]
pub struct DeckHoldCycle {
    pages: Vec<String>,
    long_press_ms: u64,
    pressed_at: Option<Instant>,
}

impl BuildFromArgs for DeckHoldCycle {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let pages = args
            .get("pages")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Config("deck.hold_cycle requires pages (array)".into()))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>();
        if pages.is_empty() {
            return Err(Error::Config("deck.hold_cycle: pages empty".into()));
        }
        let long_press_ms = args
            .get("long_press_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i as u64)
            .unwrap_or(400);
        Ok(Self {
            pages,
            long_press_ms,
            pressed_at: None,
        })
    }
}

impl Action for DeckHoldCycle {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        self.pressed_at = Some(Instant::now());
        cx.tile.flash(80);
    }

    fn on_up(&mut self, cx: &mut ActionCx<'_>) {
        let dur = self
            .pressed_at
            .take()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let direction: i64 = if dur >= self.long_press_ms { -1 } else { 1 };
        cx.bus.publish(
            "deck.cycle_request",
            serde_json::json!({
                "pages": &self.pages,
                "direction": direction,
            }),
        );
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Nav
    }
}
