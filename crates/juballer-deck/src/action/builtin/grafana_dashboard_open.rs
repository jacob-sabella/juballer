//! grafana.dashboard_open action — opens a Grafana dashboard URL.
//!
//! Args:
//!   base : string (required) — Grafana root, e.g. http://docker2.lan:3000
//!   uid  : string (required) — dashboard uid

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct GrafanaDashboardOpen {
    base: String,
    uid: String,
}

impl BuildFromArgs for GrafanaDashboardOpen {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let base = args
            .get("base")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("grafana.dashboard_open requires base".into()))?
            .to_string();
        let uid = args
            .get("uid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("grafana.dashboard_open requires uid".into()))?
            .to_string();
        Ok(Self { base, uid })
    }
}

impl Action for GrafanaDashboardOpen {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let url = format!("{}/d/{}", self.base.trim_end_matches('/'), self.uid);
        let topic = format!("action.grafana.dashboard_open:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let r = if cfg!(target_os = "windows") {
                tokio::process::Command::new("cmd")
                    .args(["/C", "start", "", &url])
                    .spawn()
            } else {
                tokio::process::Command::new("xdg-open").arg(&url).spawn()
            };
            bus.publish(
                topic,
                match r {
                    Ok(_) => serde_json::json!({"url": url}),
                    Err(e) => serde_json::json!({"error": e.to_string()}),
                },
            );
        });
        cx.tile.flash(120);
    }
}
