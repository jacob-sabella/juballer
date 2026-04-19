//! text.type_string action — types a literal string into the focused window.
//!
//! Args:
//!   text : string (required)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use enigo::{Enigo, Keyboard, Settings};

#[derive(Debug)]
pub struct TextTypeString {
    text: String,
}

impl BuildFromArgs for TextTypeString {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("text.type_string requires text".into()))?
            .to_string();
        Ok(Self { text })
    }
}

impl Action for TextTypeString {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let text = self.text.clone();
        let topic = format!("action.text.type_string:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = (|| -> std::result::Result<(), String> {
                let mut e = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
                e.text(&text).map_err(|e| e.to_string())?;
                Ok(())
            })();
            bus.publish(
                topic,
                match r {
                    Ok(()) => serde_json::json!({"ok": true, "len": text.len()}),
                    Err(e) => serde_json::json!({"error": e}),
                },
            );
        });
        cx.tile.flash(120);
    }
}
