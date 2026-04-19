//! toggle.cycle_n action — cycles a state index through `count` values.
//!
//! Args:
//!   name   : string (required)
//!   count  : u64    (required)
//!   labels : array of string (optional) — if provided, sets tile label per state

use crate::action::{Action, ActionCx, ActionKind, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct ToggleCycleN {
    name: String,
    count: u64,
    labels: Vec<String>,
}

impl BuildFromArgs for ToggleCycleN {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("toggle.cycle_n requires name".into()))?
            .to_string();
        let count = args
            .get("count")
            .and_then(|v| v.as_integer())
            .ok_or_else(|| Error::Config("toggle.cycle_n requires count".into()))?
            as u64;
        if count == 0 {
            return Err(Error::Config("count must be > 0".into()));
        }
        let labels = args
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            name,
            count,
            labels,
        })
    }
}

impl Action for ToggleCycleN {
    fn on_will_appear(&mut self, cx: &mut ActionCx<'_>) {
        let key = format!("toggle:{}", self.name);
        let i = cx
            .state
            .binding(&key)
            .and_then(|v| v.get("i"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if let Some(lbl) = self.labels.get(i as usize) {
            cx.tile.set_label(lbl);
        }
    }

    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let key = format!("toggle:{}", self.name);
        let i = cx
            .state
            .binding(&key)
            .and_then(|v| v.get("i"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let new = (i + 1) % self.count;
        cx.state.set_binding(&key, serde_json::json!({"i": new}));
        if let Some(lbl) = self.labels.get(new as usize) {
            cx.tile.set_label(lbl);
        }
        cx.bus.publish(
            format!("toggle.{}", self.name),
            serde_json::json!({"i": new}),
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
        let mut args = toml::Table::new();
        args.insert("count".into(), toml::Value::Integer(3));
        let err = ToggleCycleN::from_args(&args).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_requires_count() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("mode".into()));
        let err = ToggleCycleN::from_args(&args).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_rejects_zero_count() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("mode".into()));
        args.insert("count".into(), toml::Value::Integer(0));
        let err = ToggleCycleN::from_args(&args).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_valid() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("scene".into()));
        args.insert("count".into(), toml::Value::Integer(4));
        args.insert(
            "labels".into(),
            toml::Value::Array(vec![
                toml::Value::String("A".into()),
                toml::Value::String("B".into()),
                toml::Value::String("C".into()),
                toml::Value::String("D".into()),
            ]),
        );
        let a = ToggleCycleN::from_args(&args).unwrap();
        assert_eq!(a.count, 4);
        assert_eq!(a.labels, vec!["A", "B", "C", "D"]);
    }
}
