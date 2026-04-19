//! toggle.onoff action — flips a boolean state and renders on/off label + state_color.
//!
//! Args:
//!   name      : string (required)
//!   on_label  : string (default "on")
//!   off_label : string (default "off")
//!   on_color  : [u8;4] (default [0x23, 0xa5, 0x5a, 0xff]) — green
//!   off_color : [u8;4] (default [0x45, 0x47, 0x54, 0xff]) — gray

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::tile::TileHandle;
use crate::{Error, Result};
use juballer_core::Color;

#[derive(Debug)]
pub struct ToggleOnoff {
    name: String,
    on_label: String,
    off_label: String,
    on_color: Color,
    off_color: Color,
}

fn parse_color(v: &toml::Value, default: Color) -> Color {
    v.as_array()
        .and_then(|a| {
            if a.len() == 4 {
                Some(Color::rgba(
                    a[0].as_integer()? as u8,
                    a[1].as_integer()? as u8,
                    a[2].as_integer()? as u8,
                    a[3].as_integer()? as u8,
                ))
            } else {
                None
            }
        })
        .unwrap_or(default)
}

impl BuildFromArgs for ToggleOnoff {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("toggle.onoff requires name".into()))?
            .to_string();
        let on_label = args
            .get("on_label")
            .and_then(|v| v.as_str())
            .unwrap_or("on")
            .to_string();
        let off_label = args
            .get("off_label")
            .and_then(|v| v.as_str())
            .unwrap_or("off")
            .to_string();
        let on_color = args
            .get("on_color")
            .map(|v| parse_color(v, Color::rgba(0x23, 0xa5, 0x5a, 0xff)))
            .unwrap_or(Color::rgba(0x23, 0xa5, 0x5a, 0xff));
        let off_color = args
            .get("off_color")
            .map(|v| parse_color(v, Color::rgba(0x45, 0x47, 0x54, 0xff)))
            .unwrap_or(Color::rgba(0x45, 0x47, 0x54, 0xff));
        Ok(Self {
            name,
            on_label,
            off_label,
            on_color,
            off_color,
        })
    }
}

fn apply(tile: &mut TileHandle, on: bool, me: &ToggleOnoff) {
    if on {
        tile.set_label(&me.on_label);
        tile.set_state_color(me.on_color);
    } else {
        tile.set_label(&me.off_label);
        tile.set_state_color(me.off_color);
    }
}

impl Action for ToggleOnoff {
    fn on_will_appear(&mut self, cx: &mut ActionCx<'_>) {
        let key = format!("toggle:{}", self.name);
        let on = cx
            .state
            .binding(&key)
            .and_then(|v| v.get("on"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        apply(&mut cx.tile, on, self);
    }

    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let key = format!("toggle:{}", self.name);
        let cur = cx
            .state
            .binding(&key)
            .and_then(|v| v.get("on"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let new = !cur;
        cx.state.set_binding(&key, serde_json::json!({ "on": new }));
        apply(&mut cx.tile, new, self);
        cx.bus.publish(
            format!("toggle.{}", self.name),
            serde_json::json!({ "on": new }),
        );
        cx.tile.flash(100);
    }

    fn kind(&self) -> ActionKind {
        ActionKind::Toggle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_name() {
        let err = ToggleOnoff::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_defaults() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("mute".into()));
        let a = ToggleOnoff::from_args(&args).unwrap();
        assert_eq!(a.name, "mute");
        assert_eq!(a.on_label, "on");
        assert_eq!(a.off_label, "off");
        assert_eq!(a.on_color, Color::rgba(0x23, 0xa5, 0x5a, 0xff));
        assert_eq!(a.off_color, Color::rgba(0x45, 0x47, 0x54, 0xff));
    }

    #[test]
    fn from_args_custom_labels() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("stream".into()));
        args.insert("on_label".into(), toml::Value::String("live".into()));
        args.insert("off_label".into(), toml::Value::String("idle".into()));
        let a = ToggleOnoff::from_args(&args).unwrap();
        assert_eq!(a.on_label, "live");
        assert_eq!(a.off_label, "idle");
    }

    #[test]
    fn parse_color_fallback_on_wrong_length() {
        let v = toml::Value::Array(vec![toml::Value::Integer(255)]);
        let default = Color::rgba(1, 2, 3, 4);
        assert_eq!(parse_color(&v, default), default);
    }
}
