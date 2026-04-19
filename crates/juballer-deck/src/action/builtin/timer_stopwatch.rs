//! timer.stopwatch action — toggles a stopwatch; press starts, second press stops + reports elapsed.
//!
//! Args: (none)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;
use std::time::Instant;

#[derive(Debug, Default)]
pub struct TimerStopwatch {
    started_at: Option<Instant>,
}

impl BuildFromArgs for TimerStopwatch {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self::default())
    }
}

impl Action for TimerStopwatch {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        match self.started_at.take() {
            None => {
                self.started_at = Some(Instant::now());
                cx.tile.set_label("running");
                cx.bus.publish(
                    format!("action.timer.stopwatch:{}", cx.binding_id),
                    serde_json::json!({"event": "start"}),
                );
            }
            Some(t) => {
                let dur = t.elapsed();
                cx.tile.set_label(format!("{:.1}s", dur.as_secs_f32()));
                cx.bus.publish(
                    format!("action.timer.stopwatch:{}", cx.binding_id),
                    serde_json::json!({"event": "stop", "elapsed_ms": dur.as_millis()}),
                );
            }
        }
        cx.tile.flash(100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_no_args_needed() {
        let a = TimerStopwatch::from_args(&toml::Table::new()).unwrap();
        assert!(a.started_at.is_none());
    }
}
