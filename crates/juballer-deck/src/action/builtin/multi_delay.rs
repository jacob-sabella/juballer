//! multi.delay action — runs an action-list with delays between steps.
//!
//! Args:
//!   steps : array of table { cmd?: string, delay_ms?: u64 } — alternates cmds + sleeps

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug, Clone)]
struct Step {
    cmd: Option<String>,
    delay_ms: Option<u64>,
}

#[derive(Debug)]
pub struct MultiDelay {
    steps: Vec<Step>,
}

impl BuildFromArgs for MultiDelay {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let steps = args
            .get("steps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Config("multi.delay requires steps".into()))?
            .iter()
            .filter_map(|v| {
                let t = v.as_table()?;
                Some(Step {
                    cmd: t.get("cmd").and_then(|v| v.as_str()).map(String::from),
                    delay_ms: t
                        .get("delay_ms")
                        .and_then(|v| v.as_integer())
                        .map(|i| i as u64),
                })
            })
            .collect::<Vec<_>>();
        if steps.is_empty() {
            return Err(Error::Config("steps empty".into()));
        }
        Ok(Self { steps })
    }
}

impl Action for MultiDelay {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let steps = self.steps.clone();
        let topic = format!("action.multi.delay:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            for s in &steps {
                if let Some(d) = s.delay_ms {
                    tokio::time::sleep(std::time::Duration::from_millis(d)).await;
                }
                if let Some(cmd) = &s.cmd {
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
            }
            bus.publish(topic, serde_json::json!({"steps": steps.len(), "ok": true}));
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_steps() {
        let err = MultiDelay::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_rejects_empty_steps() {
        let mut args = toml::Table::new();
        args.insert("steps".into(), toml::Value::Array(vec![]));
        let err = MultiDelay::from_args(&args).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_valid_steps() {
        let step = {
            let mut t = toml::Table::new();
            t.insert("cmd".into(), toml::Value::String("echo hi".into()));
            t.insert("delay_ms".into(), toml::Value::Integer(200));
            toml::Value::Table(t)
        };
        let mut args = toml::Table::new();
        args.insert("steps".into(), toml::Value::Array(vec![step]));
        let a = MultiDelay::from_args(&args).unwrap();
        assert_eq!(a.steps.len(), 1);
        assert_eq!(a.steps[0].cmd.as_deref(), Some("echo hi"));
        assert_eq!(a.steps[0].delay_ms, Some(200));
    }
}
