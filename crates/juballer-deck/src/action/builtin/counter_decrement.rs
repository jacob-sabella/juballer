//! counter.decrement action — subtract from a named counter in state + publish new value.
//!
//! Args:
//!   name   : string (required) — counter name (key in state.bindings)
//!   step   : i64 (default 1)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct CounterDecrement {
    name: String,
    step: i64,
}

impl BuildFromArgs for CounterDecrement {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("counter.decrement requires name".into()))?
            .to_string();
        let step = args.get("step").and_then(|v| v.as_integer()).unwrap_or(1);
        Ok(Self { name, step })
    }
}

impl Action for CounterDecrement {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let key = format!("counter:{}", self.name);
        let cur = cx
            .state
            .binding(&key)
            .and_then(|v| v.get("n"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let new = cur.saturating_sub(self.step);
        cx.state.set_binding(&key, serde_json::json!({ "n": new }));
        cx.tile.set_label(format!("{}: {}", self.name, new));
        cx.bus.publish(
            format!("counter.{}", self.name),
            serde_json::json!({ "n": new }),
        );
        cx.tile.flash(100);
    }

    fn on_will_appear(&mut self, cx: &mut ActionCx<'_>) {
        let key = format!("counter:{}", self.name);
        let cur = cx
            .state
            .binding(&key)
            .and_then(|v| v.get("n"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        cx.tile.set_label(format!("{}: {}", self.name, cur));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_name() {
        let err = CounterDecrement::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_name_default_step() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("lives".into()));
        let a = CounterDecrement::from_args(&args).unwrap();
        assert_eq!(a.name, "lives");
        assert_eq!(a.step, 1);
    }

    #[test]
    fn from_args_accepts_custom_step() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("hp".into()));
        args.insert("step".into(), toml::Value::Integer(10));
        let a = CounterDecrement::from_args(&args).unwrap();
        assert_eq!(a.step, 10);
    }
}
