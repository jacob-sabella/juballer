//! keypress action — simulate a keyboard chord.
//!
//! Args:
//!   keys : string (required) — comma-separated key names: "ctrl,c", "alt,F4", "shift,1", etc.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

#[derive(Debug)]
pub struct Keypress {
    keys: Vec<String>,
}

impl BuildFromArgs for Keypress {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let keys = args
            .get("keys")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("keypress requires args.keys (string)".into()))?
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Err(Error::Config("keypress: keys list empty".into()));
        }
        Ok(Self { keys })
    }
}

fn parse_key(s: &str) -> Option<Key> {
    let lower = s.to_lowercase();
    Some(match lower.as_str() {
        "ctrl" | "control" => Key::Control,
        "alt" => Key::Alt,
        "shift" => Key::Shift,
        "meta" | "super" | "cmd" => Key::Meta,
        "tab" => Key::Tab,
        "enter" | "return" => Key::Return,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        f if f.starts_with('f') && f.len() <= 3 => {
            let n: u32 = f[1..].parse().ok()?;
            match n {
                1 => Key::F1,
                2 => Key::F2,
                3 => Key::F3,
                4 => Key::F4,
                5 => Key::F5,
                6 => Key::F6,
                7 => Key::F7,
                8 => Key::F8,
                9 => Key::F9,
                10 => Key::F10,
                11 => Key::F11,
                12 => Key::F12,
                _ => return None,
            }
        }
        c if c.chars().count() == 1 => Key::Unicode(c.chars().next().unwrap()),
        _ => return None,
    })
}

impl Action for Keypress {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let keys = self.keys.clone();
        let topic = format!("action.keypress:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let result: std::result::Result<(), String> = (|| {
                let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
                let parsed: Vec<Key> = keys
                    .iter()
                    .map(|s| parse_key(s).ok_or_else(|| format!("unknown key: {s}")))
                    .collect::<std::result::Result<_, _>>()?;
                // Press all in order, release in reverse.
                for k in &parsed {
                    enigo.key(*k, Direction::Press).map_err(|e| e.to_string())?;
                }
                for k in parsed.iter().rev() {
                    enigo
                        .key(*k, Direction::Release)
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            })();
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
    fn from_args_requires_keys() {
        let err = Keypress::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_rejects_empty_keys() {
        let mut args = toml::Table::new();
        args.insert("keys".into(), toml::Value::String("  ,  ".into()));
        let err = Keypress::from_args(&args).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_chord() {
        let mut args = toml::Table::new();
        args.insert("keys".into(), toml::Value::String("ctrl,c".into()));
        let a = Keypress::from_args(&args).unwrap();
        assert_eq!(a.keys, vec!["ctrl", "c"]);
    }

    #[test]
    fn parse_key_known_names() {
        assert!(matches!(parse_key("ctrl"), Some(Key::Control)));
        assert!(matches!(parse_key("Alt"), Some(Key::Alt)));
        assert!(matches!(parse_key("SHIFT"), Some(Key::Shift)));
        assert!(matches!(parse_key("f4"), Some(Key::F4)));
        assert!(matches!(parse_key("c"), Some(Key::Unicode('c'))));
    }

    #[test]
    fn parse_key_unknown_returns_none() {
        assert!(parse_key("unknownkey").is_none());
    }
}
