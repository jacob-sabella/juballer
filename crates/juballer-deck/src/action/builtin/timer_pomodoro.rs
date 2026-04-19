//! timer.pomodoro action — runs a 25min focus / 5min break cycle, publishes phase events.
//!
//! Args:
//!   focus_min : u64 (default 25)
//!   break_min : u64 (default 5)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug)]
pub struct TimerPomodoro {
    focus_min: u64,
    break_min: u64,
    running: Arc<Mutex<bool>>,
}

impl BuildFromArgs for TimerPomodoro {
    fn from_args(args: &toml::Table) -> Result<Self> {
        Ok(Self {
            focus_min: args
                .get("focus_min")
                .and_then(|v| v.as_integer())
                .map(|i| i as u64)
                .unwrap_or(25),
            break_min: args
                .get("break_min")
                .and_then(|v| v.as_integer())
                .map(|i| i as u64)
                .unwrap_or(5),
            running: Arc::new(Mutex::new(false)),
        })
    }
}

impl Action for TimerPomodoro {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let focus = Duration::from_secs(self.focus_min * 60);
        let break_d = Duration::from_secs(self.break_min * 60);
        let bus = cx.bus.clone();
        let topic = format!("action.timer.pomodoro:{}", cx.binding_id);
        let running = self.running.clone();
        if *running.lock().unwrap() {
            *running.lock().unwrap() = false;
            cx.tile.set_label("stopped");
            cx.bus
                .publish(&topic, serde_json::json!({"event": "cancel"}));
            cx.tile.flash(100);
            return;
        }
        *running.lock().unwrap() = true;
        cx.tile.set_label("focus");
        cx.rt.spawn(async move {
            bus.publish(&topic, serde_json::json!({"event": "focus_start"}));
            tokio::time::sleep(focus).await;
            if !*running.lock().unwrap() {
                return;
            }
            bus.publish(&topic, serde_json::json!({"event": "focus_end"}));
            bus.publish(&topic, serde_json::json!({"event": "break_start"}));
            tokio::time::sleep(break_d).await;
            if !*running.lock().unwrap() {
                return;
            }
            bus.publish(&topic, serde_json::json!({"event": "break_end"}));
            *running.lock().unwrap() = false;
        });
        cx.tile.flash(100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_defaults() {
        let a = TimerPomodoro::from_args(&toml::Table::new()).unwrap();
        assert_eq!(a.focus_min, 25);
        assert_eq!(a.break_min, 5);
    }

    #[test]
    fn from_args_custom() {
        let mut args = toml::Table::new();
        args.insert("focus_min".into(), toml::Value::Integer(50));
        args.insert("break_min".into(), toml::Value::Integer(10));
        let a = TimerPomodoro::from_args(&args).unwrap();
        assert_eq!(a.focus_min, 50);
        assert_eq!(a.break_min, 10);
    }
}
