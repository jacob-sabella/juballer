//! system.brightness action — adjusts backlight via brightnessctl.
//!
//! Args:
//!   delta : string (required) — e.g. "+5%", "-10%", "50%"

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct SystemBrightness {
    delta: String,
}

impl BuildFromArgs for SystemBrightness {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let delta = args
            .get("delta")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("system.brightness requires delta".into()))?
            .to_string();
        Ok(Self { delta })
    }
}

impl Action for SystemBrightness {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let delta = self.delta.clone();
        let topic = format!("action.system.brightness:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = tokio::process::Command::new("brightnessctl")
                .args(["set", &delta])
                .output()
                .await;
            bus.publish(
                topic,
                match r {
                    Ok(o) if o.status.success() => serde_json::json!({ "delta": delta }),
                    Ok(o) => serde_json::json!({ "exit": o.status.code() }),
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
                },
            );
        });
        cx.tile.flash(120);
    }
}
