//! multi.branch action — runs `if_cmd` and dispatches to `then_cmd` (success) or `else_cmd` (fail).
//!
//! Args:
//!   if_cmd   : string (required)
//!   then_cmd : string (optional)
//!   else_cmd : string (optional)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct MultiBranch {
    if_cmd: String,
    then_cmd: Option<String>,
    else_cmd: Option<String>,
}

impl BuildFromArgs for MultiBranch {
    fn from_args(args: &toml::Table) -> Result<Self> {
        Ok(Self {
            if_cmd: args
                .get("if_cmd")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Config("multi.branch requires if_cmd".into()))?
                .to_string(),
            then_cmd: args
                .get("then_cmd")
                .and_then(|v| v.as_str())
                .map(String::from),
            else_cmd: args
                .get("else_cmd")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }
}

impl Action for MultiBranch {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let if_cmd = self.if_cmd.clone();
        let then_cmd = self.then_cmd.clone();
        let else_cmd = self.else_cmd.clone();
        let topic = format!("action.multi.branch:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = if cfg!(target_os = "windows") {
                tokio::process::Command::new("cmd")
                    .args(["/C", &if_cmd])
                    .status()
                    .await
            } else {
                tokio::process::Command::new("sh")
                    .args(["-c", &if_cmd])
                    .status()
                    .await
            };
            let succ = r.map(|s| s.success()).unwrap_or(false);
            let branch = if succ { &then_cmd } else { &else_cmd };
            let mut taken: Option<&str> = None;
            if let Some(cmd) = branch {
                taken = Some(cmd);
                let _ = if cfg!(target_os = "windows") {
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
            }
            bus.publish(
                topic,
                serde_json::json!({
                    "branch": if succ { "then" } else { "else" },
                    "ran": taken,
                }),
            );
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_if_cmd() {
        let err = MultiBranch::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_optional_branches() {
        let mut args = toml::Table::new();
        args.insert("if_cmd".into(), toml::Value::String("true".into()));
        let a = MultiBranch::from_args(&args).unwrap();
        assert_eq!(a.if_cmd, "true");
        assert!(a.then_cmd.is_none());
        assert!(a.else_cmd.is_none());
    }

    #[test]
    fn from_args_full() {
        let mut args = toml::Table::new();
        args.insert(
            "if_cmd".into(),
            toml::Value::String("test -f /tmp/x".into()),
        );
        args.insert("then_cmd".into(), toml::Value::String("echo yes".into()));
        args.insert("else_cmd".into(), toml::Value::String("echo no".into()));
        let a = MultiBranch::from_args(&args).unwrap();
        assert_eq!(a.then_cmd.as_deref(), Some("echo yes"));
        assert_eq!(a.else_cmd.as_deref(), Some("echo no"));
    }
}
