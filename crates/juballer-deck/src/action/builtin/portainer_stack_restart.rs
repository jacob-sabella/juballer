//! portainer.stack_restart action — POST to Portainer's /stop + /start endpoints.
//!
//! Args:
//!   base       : string (required) — Portainer root, e.g. https://portainer.jacobsabella.com
//!   stack_id   : u64    (required)
//!   endpoint_id: u64    (required) — Portainer environment id (e.g. 4 for docker2)
//!   api_key    : string (required) — X-API-Key value

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct PortainerStackRestart {
    base: String,
    stack_id: u64,
    endpoint_id: u64,
    api_key: String,
}

impl BuildFromArgs for PortainerStackRestart {
    fn from_args(args: &toml::Table) -> Result<Self> {
        Ok(Self {
            base: args
                .get("base")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Config("portainer.stack_restart: base required".into()))?
                .to_string(),
            stack_id: args
                .get("stack_id")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| Error::Config("stack_id required".into()))?
                as u64,
            endpoint_id: args
                .get("endpoint_id")
                .and_then(|v| v.as_integer())
                .ok_or_else(|| Error::Config("endpoint_id required".into()))?
                as u64,
            api_key: args
                .get("api_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Config("api_key required".into()))?
                .to_string(),
        })
    }
}

impl Action for PortainerStackRestart {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let base = self.base.trim_end_matches('/').to_string();
        let stack_id = self.stack_id;
        let endpoint_id = self.endpoint_id;
        let api_key = self.api_key.clone();
        let topic = format!("action.portainer.stack_restart:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    bus.publish(topic, serde_json::json!({"error": e.to_string()}));
                    return;
                }
            };
            let stop_url = format!(
                "{}/api/stacks/{}/stop?endpointId={}",
                base, stack_id, endpoint_id
            );
            let start_url = format!(
                "{}/api/stacks/{}/start?endpointId={}",
                base, stack_id, endpoint_id
            );
            let stop = client
                .post(&stop_url)
                .header("X-API-Key", &api_key)
                .send()
                .await;
            if let Err(e) = stop {
                bus.publish(topic, serde_json::json!({"error": format!("stop: {e}")}));
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let start = client
                .post(&start_url)
                .header("X-API-Key", &api_key)
                .send()
                .await;
            bus.publish(
                topic,
                match start {
                    Ok(r) => {
                        serde_json::json!({"restarted": true, "start_status": r.status().as_u16()})
                    }
                    Err(e) => serde_json::json!({"error": format!("start: {e}")}),
                },
            );
        });
        cx.tile.flash(120);
    }
}
