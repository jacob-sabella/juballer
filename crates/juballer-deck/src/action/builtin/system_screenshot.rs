//! system.screenshot action — saves a screenshot to `path` (default /tmp/juballer-screenshot.png).
//!
//! Args:
//!   path : string (default "/tmp/juballer-screenshot.png")

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::Result;

#[derive(Debug)]
pub struct SystemScreenshot {
    path: String,
}

impl BuildFromArgs for SystemScreenshot {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("/tmp/juballer-screenshot.png")
            .to_string();
        Ok(Self { path })
    }
}

impl Action for SystemScreenshot {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let path = self.path.clone();
        let topic = format!("action.system.screenshot:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = tokio::process::Command::new("grim")
                .arg(&path)
                .output()
                .await;
            bus.publish(
                topic,
                match r {
                    Ok(o) if o.status.success() => serde_json::json!({ "path": path }),
                    Ok(o) => serde_json::json!({
                        "exit": o.status.code(),
                        "stderr": String::from_utf8_lossy(&o.stderr).into_owned()
                    }),
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
                },
            );
        });
        cx.tile.flash(150);
    }
}
