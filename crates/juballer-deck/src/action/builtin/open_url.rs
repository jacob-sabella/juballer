//! open.url action — invoke xdg-open (Linux) or `start` (Windows) on a URL.
//!
//! Args:
//!   url : string (required)

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

#[derive(Debug)]
pub struct OpenUrl {
    url: String,
}

impl BuildFromArgs for OpenUrl {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("open.url requires args.url (string)".into()))?
            .to_string();
        Ok(Self { url })
    }
}

impl Action for OpenUrl {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let url = self.url.clone();
        let topic = format!("action.open.url:{}", cx.binding_id);
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
                    Ok(_) => serde_json::json!({ "url": url }),
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
                },
            );
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_url() {
        let err = OpenUrl::from_args(&toml::Table::new()).unwrap_err();
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
            toml::Value::String("https://example.com".into()),
        );
        let a = OpenUrl::from_args(&args).unwrap();
        assert_eq!(a.url, "https://example.com");
    }
}
