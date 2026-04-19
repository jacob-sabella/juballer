//! shell.run — spawn a shell command on button-down.
//!
//! Args:
//!   cmd : string (required) — command line, executed via `sh -c` (unix) or `cmd /C` (windows).
//!
//! On press: spawns the command via tokio, publishes result to bus topic
//! "action.shell.run:{binding_id}" (status+stdout_len+stderr_len, or error).

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct ShellRun {
    cmd: String,
}

impl BuildFromArgs for ShellRun {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let cmd = args
            .get("cmd")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("shell.run requires args.cmd (string)".into()))?
            .to_string();
        Ok(Self { cmd })
    }
}

impl Action for ShellRun {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let cmd = self.cmd.clone();
        let topic = format!("action.shell.run:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let out = if cfg!(target_os = "windows") {
                tokio::process::Command::new("cmd")
                    .args(["/C", &cmd])
                    .output()
                    .await
            } else {
                tokio::process::Command::new("sh")
                    .args(["-c", &cmd])
                    .output()
                    .await
            };
            match out {
                Ok(o) => bus.publish(
                    topic,
                    serde_json::json!({
                        "status": o.status.code(),
                        "stdout_len": o.stdout.len(),
                        "stderr_len": o.stderr.len(),
                    }),
                ),
                Err(e) => bus.publish(topic, serde_json::json!({ "error": e.to_string() })),
            }
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_cmd() {
        let err = ShellRun::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_cmd() {
        let mut args = toml::Table::new();
        args.insert("cmd".into(), toml::Value::String("echo hi".into()));
        let a = ShellRun::from_args(&args).unwrap();
        assert_eq!(a.cmd, "echo hi");
    }
}
