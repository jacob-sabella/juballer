//! timer.countdown action — counts down N seconds, updates tile label, publishes done event.
//!
//! Args:
//!   seconds : u64 (required)
//!   label   : string (default "timer")

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct TimerCountdown {
    seconds: u64,
    label: String,
    deadline: Arc<Mutex<Option<Instant>>>,
}

impl BuildFromArgs for TimerCountdown {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let seconds = args
            .get("seconds")
            .and_then(|v| v.as_integer())
            .ok_or_else(|| Error::Config("timer.countdown requires seconds".into()))?
            as u64;
        let label = args
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("timer")
            .to_string();
        Ok(Self {
            seconds,
            label,
            deadline: Arc::new(Mutex::new(None)),
        })
    }
}

impl Action for TimerCountdown {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let secs = self.seconds;
        let deadline_until = Instant::now() + Duration::from_secs(secs);
        *self.deadline.lock().unwrap() = Some(deadline_until);
        let bus = cx.bus.clone();
        let topic = format!("action.timer.countdown:{}", cx.binding_id);
        let label = self.label.clone();
        let deadline_arc = self.deadline.clone();
        cx.tile.set_label(format!("{label} {secs}s"));
        cx.rt.spawn(async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            // Only fire if not cancelled.
            let still_active = deadline_arc
                .lock()
                .unwrap()
                .map(|d| d == deadline_until)
                .unwrap_or(false);
            if still_active {
                bus.publish(topic, serde_json::json!({"done": true, "label": label}));
            }
        });
        cx.tile.flash(100);
    }

    fn on_will_appear(&mut self, cx: &mut ActionCx<'_>) {
        cx.tile
            .set_label(format!("{} 0:0{}", self.label, self.seconds % 10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_seconds() {
        let err = TimerCountdown::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_defaults_label() {
        let mut args = toml::Table::new();
        args.insert("seconds".into(), toml::Value::Integer(10));
        let a = TimerCountdown::from_args(&args).unwrap();
        assert_eq!(a.seconds, 10);
        assert_eq!(a.label, "timer");
    }

    #[test]
    fn from_args_custom_label() {
        let mut args = toml::Table::new();
        args.insert("seconds".into(), toml::Value::Integer(30));
        args.insert("label".into(), toml::Value::String("break".into()));
        let a = TimerCountdown::from_args(&args).unwrap();
        assert_eq!(a.label, "break");
    }
}
