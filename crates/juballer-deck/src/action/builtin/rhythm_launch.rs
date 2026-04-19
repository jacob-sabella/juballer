//! rhythm.launch action — hand the current process over to rhythm mode.
//!
//! Unlike `app.launch` (which *spawns* a child), this calls `exec()` to
//! replace the running juballer-deck binary. Needed because rhythm mode
//! opens its own fullscreen window and claims the HID controller — two
//! instances would fight over both, and the deck's event loop would
//! remain running invisibly behind the rhythm window.
//!
//! Args:
//!   subcommand : string (optional) — rhythm CLI subcommand to launch.
//!     Defaults to `"play"`. Valid values mirror the top-level CLI:
//!     `play`, `calibrate-audio`, `tutorial`, `settings`.
//!   chart      : string (optional) — only meaningful for `subcommand =
//!     "play"`. Absolute or CWD-relative path to a `.memon` file or a
//!     directory of them (directory → picker). When omitted with
//!     `subcommand = "play"`, the deck falls back to `rhythm.charts_dir`
//!     from the config.
//!   difficulty : string (optional) — `--difficulty` flag for `play`.
//!     Default BSC (matches the CLI default).
//!   audio_offset_ms : integer (optional) — `--audio-offset-ms` flag.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use std::os::unix::process::CommandExt;

#[derive(Debug)]
pub struct RhythmLaunch {
    subcommand: String,
    chart: Option<String>,
    difficulty: Option<String>,
    audio_offset_ms: Option<i64>,
}

const DEFAULT_SUBCOMMAND: &str = "play";
const VALID_SUBCOMMANDS: &[&str] = &["play", "calibrate-audio", "tutorial", "settings", "mods"];

impl BuildFromArgs for RhythmLaunch {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let subcommand = args
            .get("subcommand")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_SUBCOMMAND)
            .to_string();
        if !VALID_SUBCOMMANDS.contains(&subcommand.as_str()) {
            return Err(Error::Config(format!(
                "rhythm.launch: unknown subcommand {subcommand:?}; expected one of {VALID_SUBCOMMANDS:?}"
            )));
        }
        let chart = args
            .get("chart")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let difficulty = args
            .get("difficulty")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let audio_offset_ms = args.get("audio_offset_ms").and_then(|v| v.as_integer());
        Ok(Self {
            subcommand,
            chart,
            difficulty,
            audio_offset_ms,
        })
    }
}

impl Action for RhythmLaunch {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        // Build the argv for the next process. `exec()` on success never
        // returns; on failure we report via the event bus so the editor /
        // debugger sees why we couldn't take over.
        let topic = format!("action.rhythm.launch:{}", cx.binding_id);
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

        let mut argv: Vec<String> = vec![self.subcommand.clone()];
        match self.subcommand.as_str() {
            "play" => {
                if let Some(c) = &self.chart {
                    argv.push(c.clone());
                }
                if let Some(d) = &self.difficulty {
                    argv.push("--difficulty".to_string());
                    argv.push(d.clone());
                }
                if let Some(off) = self.audio_offset_ms {
                    argv.push("--audio-offset-ms".to_string());
                    argv.push(off.to_string());
                }
            }
            "calibrate-audio" | "tutorial" => {
                if let Some(off) = self.audio_offset_ms {
                    argv.push("--audio-offset-ms".to_string());
                    argv.push(off.to_string());
                }
            }
            _ => {}
        }

        bus.publish(
            topic.clone(),
            serde_json::json!({
                "exec": exe.display().to_string(),
                "argv": argv,
            }),
        );

        // Tell the child that when it graceful-exits, it should re-exec
        // the deck binary (no args → DeckApp) so the user lands back in
        // the deck surface they launched from. Honoured by rhythm::exit.
        let err = std::process::Command::new(&exe)
            .args(&argv)
            .env("JUBALLER_RETURN_TO", "deck")
            .exec();
        bus.publish(topic, serde_json::json!({ "error": err.to_string() }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl(entries: &[(&str, toml::Value)]) -> toml::Table {
        let mut t = toml::Table::new();
        for (k, v) in entries {
            t.insert((*k).to_string(), v.clone());
        }
        t
    }

    #[test]
    fn defaults_to_play_subcommand() {
        let a = RhythmLaunch::from_args(&toml::Table::new()).unwrap();
        assert_eq!(a.subcommand, "play");
        assert!(a.chart.is_none());
    }

    #[test]
    fn parses_play_with_all_args() {
        let args = tbl(&[
            ("subcommand", toml::Value::String("play".into())),
            ("chart", toml::Value::String("/charts/demo.memon".into())),
            ("difficulty", toml::Value::String("ADV".into())),
            ("audio_offset_ms", toml::Value::Integer(-12)),
        ]);
        let a = RhythmLaunch::from_args(&args).unwrap();
        assert_eq!(a.subcommand, "play");
        assert_eq!(a.chart.as_deref(), Some("/charts/demo.memon"));
        assert_eq!(a.difficulty.as_deref(), Some("ADV"));
        assert_eq!(a.audio_offset_ms, Some(-12));
    }

    #[test]
    fn rejects_unknown_subcommand() {
        let args = tbl(&[("subcommand", toml::Value::String("nope".into()))]);
        let err = RhythmLaunch::from_args(&args).unwrap_err();
        match err {
            Error::Config(msg) => assert!(msg.contains("nope")),
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
