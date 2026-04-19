//! http.post action — POST with optional JSON or form body.
//!
//! Args:
//!   url     : string (required)
//!   headers : table (optional)
//!   json    : value (optional)  — sent as JSON body
//!   body    : string (optional) — sent as raw text body if json absent

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use std::collections::HashMap;

#[derive(Debug)]
pub struct HttpPost {
    url: String,
    headers: HashMap<String, String>,
    json: Option<serde_json::Value>,
    body: Option<String>,
}

impl BuildFromArgs for HttpPost {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("http.post requires args.url".into()))?
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
        // toml::Value → serde_json::Value via to_string + parse, simple path.
        let json = args.get("json").map(|v| {
            let s = toml::to_string(v).unwrap_or_default();
            serde_json::from_str::<serde_json::Value>(&s).unwrap_or_else(|_| serde_json::json!(s))
        });
        let body = args.get("body").and_then(|v| v.as_str()).map(String::from);
        Ok(Self {
            url,
            headers,
            json,
            body,
        })
    }
}

impl Action for HttpPost {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let url = self.url.clone();
        let headers = self.headers.clone();
        let json = self.json.clone();
        let body = self.body.clone();
        let topic = format!("action.http.post:{}", cx.binding_id);
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
            let mut req = client.post(&url);
            for (k, v) in &headers {
                req = req.header(k, v);
            }
            if let Some(j) = json {
                req = req.json(&j);
            } else if let Some(b) = body {
                req = req.body(b);
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
        let err = HttpPost::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_url_only() {
        let mut args = toml::Table::new();
        args.insert(
            "url".into(),
            toml::Value::String("http://example.com".into()),
        );
        let a = HttpPost::from_args(&args).unwrap();
        assert_eq!(a.url, "http://example.com");
        assert!(a.headers.is_empty());
        assert!(a.json.is_none());
        assert!(a.body.is_none());
    }

    #[test]
    fn from_args_accepts_body() {
        let mut args = toml::Table::new();
        args.insert(
            "url".into(),
            toml::Value::String("http://example.com".into()),
        );
        args.insert("body".into(), toml::Value::String("hello".into()));
        let a = HttpPost::from_args(&args).unwrap();
        assert_eq!(a.body.as_deref(), Some("hello"));
    }
}
