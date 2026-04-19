//! text.snippet_expand action — type a snippet template, substituting current date/time.
//!
//! Args:
//!   template : string (required)
//!     supports {date}, {time}, {datetime}, {uuid} placeholders.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use enigo::{Enigo, Keyboard, Settings};

#[derive(Debug)]
pub struct TextSnippetExpand {
    template: String,
}

impl BuildFromArgs for TextSnippetExpand {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let template = args
            .get("template")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("text.snippet_expand requires template".into()))?
            .to_string();
        Ok(Self { template })
    }
}

fn expand(template: &str) -> String {
    let now = chrono::Local::now();
    let mut s = template.to_string();
    s = s.replace("{date}", &now.format("%Y-%m-%d").to_string());
    s = s.replace("{time}", &now.format("%H:%M:%S").to_string());
    s = s.replace("{datetime}", &now.format("%Y-%m-%d %H:%M:%S").to_string());
    if s.contains("{uuid}") {
        // Simple pseudo-uuid via timestamp + random bits.
        let bits = now.timestamp_millis();
        s = s.replace("{uuid}", &format!("{:016x}", bits));
    }
    s
}

impl Action for TextSnippetExpand {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let expanded = expand(&self.template);
        let topic = format!("action.text.snippet_expand:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = (|| -> std::result::Result<(), String> {
                let mut e = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
                e.text(&expanded).map_err(|e| e.to_string())?;
                Ok(())
            })();
            bus.publish(
                topic,
                match r {
                    Ok(()) => serde_json::json!({"ok": true, "expanded": expanded}),
                    Err(e) => serde_json::json!({"error": e}),
                },
            );
        });
        cx.tile.flash(120);
    }
}
