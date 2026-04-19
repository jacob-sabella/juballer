//! multi.run_list action — fire a list of shell commands sequentially.
//!
//! Args:
//!   cmds : array of string (required)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct MultiRunList {
    cmds: Vec<String>,
}

impl BuildFromArgs for MultiRunList {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let cmds = args
            .get("cmds")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Config("multi.run_list requires cmds (array)".into()))?
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>();
        if cmds.is_empty() {
            return Err(Error::Config("multi.run_list: empty cmds".into()));
        }
        Ok(Self { cmds })
    }
}

impl Action for MultiRunList {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let cmds = self.cmds.clone();
        let topic = format!("action.multi.run_list:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let mut results = Vec::new();
            for cmd in &cmds {
                let r = if cfg!(target_os = "windows") {
                    tokio::process::Command::new("cmd")
                        .args(["/C", cmd])
                        .output()
                        .await
                } else {
                    tokio::process::Command::new("sh")
                        .args(["-c", cmd])
                        .output()
                        .await
                };
                results.push(serde_json::json!({
                    "cmd": cmd,
                    "ok": r.as_ref().map(|o| o.status.success()).unwrap_or(false),
                }));
            }
            bus.publish(topic, serde_json::json!({ "steps": results }));
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_cmds() {
        let err = MultiRunList::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_rejects_empty_array() {
        let mut args = toml::Table::new();
        args.insert("cmds".into(), toml::Value::Array(vec![]));
        let err = MultiRunList::from_args(&args).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_cmds() {
        let mut args = toml::Table::new();
        args.insert(
            "cmds".into(),
            toml::Value::Array(vec![
                toml::Value::String("echo a".into()),
                toml::Value::String("echo b".into()),
            ]),
        );
        let a = MultiRunList::from_args(&args).unwrap();
        assert_eq!(a.cmds, vec!["echo a", "echo b"]);
    }
}
