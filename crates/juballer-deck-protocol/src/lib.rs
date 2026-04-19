//! Wire-format types for juballer-deck ↔ plugin IPC.
//! NDJSON over UDS (Linux) / named pipe (Windows).
#![forbid(unsafe_op_in_unsafe_fn)]

use serde::{Deserialize, Serialize};

pub mod view;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Handshake — sent both directions on connect.
    Hello {
        v: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        deck_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        plugin: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        plugin_version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sdk: Option<String>,
    },
    /// Deck → plugin: ready to receive lifecycle messages.
    RegisterComplete,
    /// Heartbeat ping (deck → plugin).
    Ping,
    /// Pong reply (plugin → deck).
    Pong,
    /// Deck → plugin: action instance is being shown.
    WillAppear {
        action: String,
        binding_id: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    /// Deck → plugin: action being torn down.
    WillDisappear { binding_id: String },
    /// Deck → plugin: button pressed.
    KeyDown { binding_id: String },
    /// Deck → plugin: button released.
    KeyUp { binding_id: String },
    /// Deck → plugin: widget being shown.
    WidgetWillAppear {
        widget: String,
        pane_id: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    /// Deck → plugin: widget torn down.
    WidgetWillDisappear { pane_id: String },
    /// Deck → plugin: bus event for subscribed topic.
    Event {
        topic: String,
        #[serde(default)]
        data: serde_json::Value,
    },
    /// Plugin → deck: update tile state.
    TileSet {
        binding_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_color: Option<String>, // hex like "#23a55a"
    },
    /// Plugin → deck: brief tile flash.
    TileFlash { binding_id: String, ms: u16 },
    /// Plugin → deck: update a tile by its logical `name` (from `ButtonCfg.name`).
    /// Scroll-invariant: named tiles are located each frame regardless of page scroll.
    /// Omitted fields preserve the tile's existing value; `clear = true` resets to
    /// the config-default (no plugin-supplied overrides).
    TileSetByName {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clear: Option<bool>,
    },
    /// Plugin → deck: declarative widget content.
    WidgetSet {
        pane_id: String,
        #[serde(default)]
        content: serde_json::Value,
    },
    /// Plugin → deck: push a structured view tree for a `dynamic` widget pane.
    WidgetViewUpdate { pane: String, tree: view::ViewNode },
    /// Plugin → deck: publish to bus.
    BusPublish {
        topic: String,
        #[serde(default)]
        data: serde_json::Value,
    },
    /// Plugin → deck: subscribe to bus topics (prefix-match).
    BusSubscribe { topics: Vec<String> },
    /// Plugin → deck: log line.
    Log { level: String, msg: String },
    /// Plugin → deck: structured error.
    Error { code: String, msg: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_roundtrip() {
        let m = Message::Hello {
            v: PROTOCOL_VERSION,
            deck_version: Some("0.1.0".into()),
            plugin: None,
            plugin_version: None,
            sdk: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: Message = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, Message::Hello { v, .. } if v == 1));
    }

    #[test]
    fn key_down_roundtrip() {
        let m = Message::KeyDown {
            binding_id: "home:0,1".into(),
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"type\":\"key_down\""));
        assert!(s.contains("home:0,1"));
        let back: Message = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, Message::KeyDown { ref binding_id } if binding_id == "home:0,1"));
    }

    #[test]
    fn widget_view_update_roundtrip() {
        let m = Message::WidgetViewUpdate {
            pane: "discord_pane".into(),
            tree: view::ViewNode::Vstack {
                gap: 4.0,
                align: view::Align::Start,
                children: vec![view::ViewNode::Text {
                    value: "hi".into(),
                    size: None,
                    color: None,
                    weight: None,
                }],
            },
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"type\":\"widget_view_update\""));
        assert!(s.contains("\"pane\":\"discord_pane\""));
        let back: Message = serde_json::from_str(&s).unwrap();
        match back {
            Message::WidgetViewUpdate { pane, tree } => {
                assert_eq!(pane, "discord_pane");
                assert!(matches!(tree, view::ViewNode::Vstack { .. }));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tile_set_by_name_roundtrip() {
        let m = Message::TileSetByName {
            name: "discord_unread".into(),
            icon: Some("💬".into()),
            label: Some("3 DM".into()),
            state_color: Some("red".into()),
            clear: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"type\":\"tile_set_by_name\""));
        assert!(s.contains("\"name\":\"discord_unread\""));
        assert!(s.contains("\"state_color\":\"red\""));
        assert!(!s.contains("\"clear\""));
        let back: Message = serde_json::from_str(&s).unwrap();
        match back {
            Message::TileSetByName {
                name,
                icon,
                label,
                state_color,
                clear,
            } => {
                assert_eq!(name, "discord_unread");
                assert_eq!(icon.as_deref(), Some("💬"));
                assert_eq!(label.as_deref(), Some("3 DM"));
                assert_eq!(state_color.as_deref(), Some("red"));
                assert!(clear.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tile_set_by_name_clear_only() {
        let json = r#"{"type":"tile_set_by_name","name":"x","clear":true}"#;
        let m: Message = serde_json::from_str(json).unwrap();
        match m {
            Message::TileSetByName {
                name,
                clear,
                icon,
                label,
                state_color,
            } => {
                assert_eq!(name, "x");
                assert_eq!(clear, Some(true));
                assert!(icon.is_none());
                assert!(label.is_none());
                assert!(state_color.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn tile_set_partial() {
        let json = r#"{"type":"tile_set","binding_id":"x","label":"hi"}"#;
        let m: Message = serde_json::from_str(json).unwrap();
        match m {
            Message::TileSet {
                binding_id,
                label,
                icon,
                state_color,
            } => {
                assert_eq!(binding_id, "x");
                assert_eq!(label.unwrap(), "hi");
                assert!(icon.is_none());
                assert!(state_color.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }
}
