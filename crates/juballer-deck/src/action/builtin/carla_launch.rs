//! `carla.launch` action — hand the current process over to carla mode.
//!
//! Mirrors `rhythm.launch`: re-execs the deck binary with the `carla`
//! subcommand so the new process owns the fullscreen window + the HID
//! controller. Two instances would fight over both, and the deck's
//! event loop would otherwise stay running invisibly behind the carla
//! window.
//!
//! Args:
//! - `config` : string (optional) — absolute or CWD-relative path to a
//!   carla configuration TOML. When omitted the carla subcommand picks
//!   the alphabetically-first file from the default config dir.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug)]
pub struct CarlaLaunch {
    config: Option<String>,
}

impl BuildFromArgs for CarlaLaunch {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let config = args
            .get("config")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(Self { config })
    }
}

impl Action for CarlaLaunch {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let topic = format!("action.carla.launch:{}", cx.binding_id);
        let bus = cx.bus.clone();

        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                bus.publish(
                    topic,
                    serde_json::json!({ "error": format!("current_exe: {e}") }),
                );
                return;
            }
        };

        let mut argv: Vec<String> = vec!["carla".to_string()];
        if let Some(c) = &self.config {
            argv.push(c.clone());
        }

        bus.publish(
            topic.clone(),
            serde_json::json!({
                "exec": exe.display().to_string(),
                "argv": argv,
            }),
        );

        let mut cmd = std::process::Command::new(&exe);
        cmd.args(&argv).env("JUBALLER_RETURN_TO", "deck");
        let err = relaunch_or_spawn(cmd);
        bus.publish(topic, serde_json::json!({ "error": err }));
    }
}

/// See `rhythm_launch::relaunch_or_spawn` — same Unix vs Windows split.
/// Duplicated rather than shared because the relaunch path is one of
/// the few places where the per-action wiring justifies its own
/// transparent in-place exec story; pulling it into a shared helper
/// hides the platform-specific implications behind a layer of
/// indirection that is not earned by the two callers.
fn relaunch_or_spawn(mut cmd: std::process::Command) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.exec().to_string()
    }
    #[cfg(not(unix))]
    {
        match cmd.spawn() {
            Ok(_) => {
                std::process::exit(0);
            }
            Err(e) => e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_with_no_config_keeps_field_none() {
        let a = CarlaLaunch::from_args(&toml::Table::new()).unwrap();
        assert!(a.config.is_none());
    }

    #[test]
    fn from_args_with_config_path_records_it() {
        let mut t = toml::Table::new();
        t.insert(
            "config".into(),
            toml::Value::String("/cfg/drums.toml".into()),
        );
        let a = CarlaLaunch::from_args(&t).unwrap();
        assert_eq!(a.config.as_deref(), Some("/cfg/drums.toml"));
    }
}
