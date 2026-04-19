//! http.get action — fire-and-forget HTTP GET.
//!
//! Args:
//!   url     : string (required)
//!   headers : table of string→string (optional)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use std::collections::HashMap;

#[derive(Debug)]
pub struct HttpGet {
    url: String,
    headers: HashMap<String, String>,
}

impl BuildFromArgs for HttpGet {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("http.get requires args.url".into()))?
            .to_string();
        let headers = args
            .get("headers")
            .and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { url, headers })
    }
}

impl Action for HttpGet {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let url = self.url.clone();
        let headers = self.headers.clone();
        let topic = format!("action.http.get:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    bus.publish(topic, serde_json::json!({"error": e.to_string()}));
                    return;
                }
            };
            let mut req = client.get(&url);
            for (k, v) in &headers {
                req = req.header(k, v);
            }
            match req.send().await {
                Ok(r) => bus.publish(
                    topic,
                    serde_json::json!({"status": r.status().as_u16(), "url": url}),
                ),
                Err(e) => bus.publish(
                    topic,
                    serde_json::json!({"error": e.to_string(), "url": url}),
                ),
            }
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_url() {
        let err = HttpGet::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_url() {
        let mut args = toml::Table::new();
        args.insert(
            "url".into(),
            toml::Value::String("http://example.com".into()),
        );
        let a = HttpGet::from_args(&args).unwrap();
        assert_eq!(a.url, "http://example.com");
        assert!(a.headers.is_empty());
    }
}
