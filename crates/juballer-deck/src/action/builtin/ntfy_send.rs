//! ntfy.send action — POST a message to an ntfy topic.
//!
//! Args:
//!   server   : string (default "http://docker2.lan:8555")
//!   topic    : string (required)
//!   message  : string (required)
//!   user     : string (optional) — basic auth user
//!   pass     : string (optional) — basic auth pass
//!   priority : string (optional, "min"|"low"|"default"|"high"|"max")

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct NtfySend {
    server: String,
    topic: String,
    message: String,
    user: Option<String>,
    pass: Option<String>,
    priority: Option<String>,
}

impl BuildFromArgs for NtfySend {
    fn from_args(args: &toml::Table) -> Result<Self> {
        Ok(Self {
            server: args
                .get("server")
                .and_then(|v| v.as_str())
                .unwrap_or("http://docker2.lan:8555")
                .to_string(),
            topic: args
                .get("topic")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Config("ntfy.send requires topic".into()))?
                .to_string(),
            message: args
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Config("ntfy.send requires message".into()))?
                .to_string(),
            user: args.get("user").and_then(|v| v.as_str()).map(String::from),
            pass: args.get("pass").and_then(|v| v.as_str()).map(String::from),
            priority: args
                .get("priority")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }
}

impl Action for NtfySend {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let url = format!("{}/{}", self.server.trim_end_matches('/'), self.topic);
        let body = self.message.clone();
        let user = self.user.clone();
        let pass = self.pass.clone();
        let priority = self.priority.clone();
        let topic_pub = format!("action.ntfy.send:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    bus.publish(topic_pub, serde_json::json!({"error": e.to_string()}));
                    return;
                }
            };
            let mut req = client.post(&url).body(body);
            if let (Some(u), Some(p)) = (user.as_ref(), pass.as_ref()) {
                req = req.basic_auth(u, Some(p));
            }
            if let Some(p) = priority {
                req = req.header("Priority", p);
            }
            match req.send().await {
                Ok(r) => bus.publish(
                    topic_pub,
                    serde_json::json!({"status": r.status().as_u16()}),
                ),
                Err(e) => bus.publish(topic_pub, serde_json::json!({"error": e.to_string()})),
            }
        });
        cx.tile.flash(120);
    }
}
