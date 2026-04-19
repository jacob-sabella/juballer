//! system.mic_mute action — toggles microphone mute via pactl.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug, Default)]
pub struct SystemMicMute;

impl BuildFromArgs for SystemMicMute {
    fn from_args(_args: &toml::Table) -> Result<Self> {
        Ok(Self)
    }
}

impl Action for SystemMicMute {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let topic = format!("action.system.mic_mute:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = tokio::process::Command::new("pactl")
                .args(["set-source-mute", "@DEFAULT_SOURCE@", "toggle"])
                .output()
                .await;
            bus.publish(
                topic,
                match r {
                    Ok(o) if o.status.success() => serde_json::json!({ "toggled": true }),
                    Ok(o) => serde_json::json!({ "exit": o.status.code() }),
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
                },
            );
        });
        cx.tile.flash(120);
    }
}
