//! plugin_proxy_action — generic action that delegates to a plugin via the deck's
//! plugin host. Built dynamically from a plugin manifest's `actions` list.
//!
//! The deck wire-up code calls `register_plugin_sender` once per plugin after
//! spawning plugins. Each `PluginProxyAction` instance carries the action name
//! + which plugin owns it.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};
use juballer_deck_protocol::Message;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use tokio::sync::mpsc;

/// Global registry: plugin name → outbound message channel.
/// Set once by the deck after `PluginHost::spawn_all`.
static PLUGIN_SENDERS: OnceLock<RwLock<HashMap<String, mpsc::Sender<Message>>>> = OnceLock::new();

pub fn senders() -> &'static RwLock<HashMap<String, mpsc::Sender<Message>>> {
    PLUGIN_SENDERS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn register_plugin_sender(plugin: String, send: mpsc::Sender<Message>) {
    senders().write().unwrap().insert(plugin, send);
}

#[derive(Debug, Clone)]
pub struct PluginProxyAction {
    pub plugin_name: String,
    pub action_name: String,
    pub args: serde_json::Value,
}

impl PluginProxyAction {
    pub fn new(plugin_name: String, action_name: String, args: serde_json::Value) -> Self {
        Self {
            plugin_name,
            action_name,
            args,
        }
    }

    fn send(&self, cx: &mut ActionCx<'_>, msg: Message) {
        let map = senders().read().unwrap();
        if let Some(tx) = map.get(&self.plugin_name) {
            let tx = tx.clone();
            cx.rt.spawn(async move {
                let _ = tx.send(msg).await;
            });
        }
    }
}

impl BuildFromArgs for PluginProxyAction {
    /// `from_args` is meaningless for the proxy — plugin actions are constructed
    /// by the deck's plugin wire-up code with already-known plugin_name +
    /// action_name. This just exists for trait completeness.
    fn from_args(args: &toml::Table) -> Result<Self> {
        let plugin_name = args
            .get("__plugin")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("plugin proxy: __plugin arg missing".into()))?
            .to_string();
        let action_name = args
            .get("__action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("plugin proxy: __action arg missing".into()))?
            .to_string();
        let json_args = args
            .iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.to_string())))
            .collect::<serde_json::Map<_, _>>();
        Ok(Self {
            plugin_name,
            action_name,
            args: serde_json::Value::Object(json_args),
        })
    }
}

impl Action for PluginProxyAction {
    fn on_will_appear(&mut self, cx: &mut ActionCx<'_>) {
        let msg = Message::WillAppear {
            action: self.action_name.clone(),
            binding_id: cx.binding_id.to_string(),
            args: self.args.clone(),
        };
        self.send(cx, msg);
    }

    fn on_will_disappear(&mut self, cx: &mut ActionCx<'_>) {
        let msg = Message::WillDisappear {
            binding_id: cx.binding_id.to_string(),
        };
        self.send(cx, msg);
    }

    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let msg = Message::KeyDown {
            binding_id: cx.binding_id.to_string(),
        };
        self.send(cx, msg);
        cx.tile.flash(80);
    }

    fn on_up(&mut self, cx: &mut ActionCx<'_>) {
        let msg = Message::KeyUp {
            binding_id: cx.binding_id.to_string(),
        };
        self.send(cx, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_meta_keys() {
        let err = PluginProxyAction::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn from_args_extracts_plugin_and_action() {
        let mut args = toml::Table::new();
        args.insert("__plugin".into(), toml::Value::String("discord".into()));
        args.insert(
            "__action".into(),
            toml::Value::String("discord.mute".into()),
        );
        let p = PluginProxyAction::from_args(&args).unwrap();
        assert_eq!(p.plugin_name, "discord");
        assert_eq!(p.action_name, "discord.mute");
    }
}
