//! text.md_template action — types a markdown snippet (heading, list, code block).
//!
//! Args:
//!   kind : string (required) — "h2" | "h3" | "list" | "code"
//!   text : string (default "") — content placed inside the template

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use enigo::{Enigo, Keyboard, Settings};

#[derive(Debug)]
pub struct TextMdTemplate {
    kind: String,
    text: String,
}

impl BuildFromArgs for TextMdTemplate {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("text.md_template requires kind".into()))?
            .to_string();
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Self { kind, text })
    }
}

fn render(kind: &str, text: &str) -> String {
    match kind {
        "h2" => format!("## {text}\n\n"),
        "h3" => format!("### {text}\n\n"),
        "list" => format!("- {text}\n"),
        "code" => format!("```\n{text}\n```\n"),
        _ => text.to_string(),
    }
}

impl Action for TextMdTemplate {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let body = render(&self.kind, &self.text);
        let topic = format!("action.text.md_template:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = (|| -> std::result::Result<(), String> {
                let mut e = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
                e.text(&body).map_err(|e| e.to_string())?;
                Ok(())
            })();
            bus.publish(
                topic,
                match r {
                    Ok(()) => serde_json::json!({"ok": true, "len": body.len()}),
                    Err(e) => serde_json::json!({"error": e}),
                },
            );
        });
        cx.tile.flash(120);
    }
}
