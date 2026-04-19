//! http.probe action — GET, optionally check status code range, publish ok/fail.
//!
//! Args:
//!   url           : string (required)
//!   ok_min, ok_max : u16 (optional, default 200..400)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct HttpProbe {
    url: String,
    ok_min: u16,
    ok_max: u16,
}

impl BuildFromArgs for HttpProbe {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("http.probe requires args.url".into()))?
            .to_string();
        let ok_min = args
            .get("ok_min")
            .and_then(|v| v.as_integer())
            .map(|i| i as u16)
            .unwrap_or(200);
        let ok_max = args
            .get("ok_max")
            .and_then(|v| v.as_integer())
            .map(|i| i as u16)
            .unwrap_or(400);
        Ok(Self {
            url,
            ok_min,
            ok_max,
        })
    }
}

impl Action for HttpProbe {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let url = self.url.clone();
        let (ok_min, ok_max) = (self.ok_min, self.ok_max);
        let topic = format!("action.http.probe:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    bus.publish(
                        topic,
                        serde_json::json!({"error": e.to_string(), "ok": false}),
                    );
                    return;
                }
            };
            match client.get(&url).send().await {
                Ok(r) => {
                    let code = r.status().as_u16();
                    let ok = (ok_min..ok_max).contains(&code);
                    bus.publish(
                        topic,
                        serde_json::json!({"status": code, "ok": ok, "url": url}),
                    );
                }
                Err(e) => bus.publish(
                    topic,
                    serde_json::json!({"error": e.to_string(), "ok": false, "url": url}),
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
        let err = HttpProbe::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_defaults() {
        let mut args = toml::Table::new();
        args.insert(
            "url".into(),
            toml::Value::String("http://example.com".into()),
        );
        let a = HttpProbe::from_args(&args).unwrap();
        assert_eq!(a.ok_min, 200);
        assert_eq!(a.ok_max, 400);
    }

    #[test]
    fn from_args_custom_range() {
        let mut args = toml::Table::new();
        args.insert(
            "url".into(),
            toml::Value::String("http://example.com".into()),
        );
        args.insert("ok_min".into(), toml::Value::Integer(200));
        args.insert("ok_max".into(), toml::Value::Integer(300));
        let a = HttpProbe::from_args(&args).unwrap();
        assert_eq!(a.ok_min, 200);
        assert_eq!(a.ok_max, 300);
    }
}
