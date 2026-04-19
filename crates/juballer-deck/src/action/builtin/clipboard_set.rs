//! clipboard.set action — write text to system clipboard.
//!
//! Args:
//!   text : string (required)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct ClipboardSet {
    text: String,
}

impl BuildFromArgs for ClipboardSet {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("clipboard.set requires args.text (string)".into()))?
            .to_string();
        Ok(Self { text })
    }
}

impl Action for ClipboardSet {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let text = self.text.clone();
        let topic = format!("action.clipboard.set:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let result = std::thread::spawn(move || {
                arboard::Clipboard::new()
                    .and_then(|mut c| c.set_text(text))
                    .map_err(|e| e.to_string())
            })
            .join()
            .map_err(|_| "clipboard thread panicked".to_string())
            .and_then(|r| r);
            bus.publish(
                topic,
                match result {
                    Ok(()) => serde_json::json!({ "ok": true }),
                    Err(e) => serde_json::json!({ "error": e }),
                },
            );
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_text() {
        let err = ClipboardSet::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_text() {
        let mut args = toml::Table::new();
        args.insert("text".into(), toml::Value::String("hello world".into()));
        let a = ClipboardSet::from_args(&args).unwrap();
        assert_eq!(a.text, "hello world");
    }

    #[test]
    fn from_args_accepts_empty_string() {
        let mut args = toml::Table::new();
        args.insert("text".into(), toml::Value::String(String::new()));
        let a = ClipboardSet::from_args(&args).unwrap();
        assert_eq!(a.text, "");
    }
}
