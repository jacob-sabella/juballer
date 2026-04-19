//! counter.display action — shows a counter's value on the tile label; press no-op.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct CounterDisplay {
    name: String,
}

impl BuildFromArgs for CounterDisplay {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("counter.display requires name".into()))?
            .to_string();
        Ok(Self { name })
    }
}

impl Action for CounterDisplay {
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

    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        cx.tile.flash(60);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_name() {
        let err = CounterDisplay::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_name() {
        let mut args = toml::Table::new();
        args.insert("name".into(), toml::Value::String("score".into()));
        let a = CounterDisplay::from_args(&args).unwrap();
        assert_eq!(a.name, "score");
    }
}
