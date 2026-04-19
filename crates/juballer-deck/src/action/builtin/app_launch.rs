//! app.launch action — spawn an executable in the background, fire-and-forget.
//!
//! Args:
//!   exe  : string (required) — executable name or path
//!   args : array of string (optional) — argv

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct AppLaunch {
    exe: String,
    args: Vec<String>,
}

impl BuildFromArgs for AppLaunch {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let exe = args
            .get("exe")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("app.launch requires args.exe (string)".into()))?
            .to_string();
        let cmd_args = args
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            exe,
            args: cmd_args,
        })
    }
}

impl Action for AppLaunch {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let exe = self.exe.clone();
        let args = self.args.clone();
        let topic = format!("action.app.launch:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = tokio::process::Command::new(&exe)
                .args(&args)
                .spawn()
                .map(|c| c.id());
            bus.publish(
                topic,
                match r {
                    Ok(Some(pid)) => serde_json::json!({ "pid": pid }),
                    Ok(None) => serde_json::json!({ "spawned": true }),
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
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
    fn from_args_requires_exe() {
        let err = AppLaunch::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_exe_only() {
        let mut args = toml::Table::new();
        args.insert("exe".into(), toml::Value::String("firefox".into()));
        let a = AppLaunch::from_args(&args).unwrap();
        assert_eq!(a.exe, "firefox");
        assert!(a.args.is_empty());
    }

    #[test]
    fn from_args_accepts_exe_with_args() {
        let mut args = toml::Table::new();
        args.insert("exe".into(), toml::Value::String("code".into()));
        args.insert(
            "args".into(),
            toml::Value::Array(vec![
                toml::Value::String("--new-window".into()),
                toml::Value::String("/tmp".into()),
            ]),
        );
        let a = AppLaunch::from_args(&args).unwrap();
        assert_eq!(a.exe, "code");
        assert_eq!(a.args, vec!["--new-window", "/tmp"]);
    }
}
