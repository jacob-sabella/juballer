use super::*;
use crate::widget::WidgetRegistry;
use serde_json::json;

/// Register every built-in widget along with its JSON Schema (Draft-07).
///
/// The web editor pulls these schemas via `WidgetRegistry::schema_for` to auto-generate
/// config forms. When adding a new widget, register it with `register_with_schema` and
/// author a schema that mirrors the TOML args the widget's `WidgetBuildFromArgs` reads.
pub fn register_builtins(registry: &mut WidgetRegistry) {
    registry.register_with_schema::<action_mini::ActionMiniWidget>(
        "action_mini",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Action button",
            "description": "Renders a tile-style button in a top-region pane. Fires the named action when clicked.",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action name that must exist in the action registry."
                },
                "icon": {
                    "type": "string",
                    "description": "Emoji or asset path. Short strings (<= 3 chars) are treated as emoji."
                },
                "label": {
                    "type": "string",
                    "description": "Optional button label. Defaults to the action name when omitted."
                },
                "args": {
                    "type": "object",
                    "description": "Arguments forwarded to the dispatched action.",
                    "additionalProperties": true,
                    "default": {}
                }
            },
            "required": ["action"]
        }),
    );

    registry.register_with_schema::<clock::Clock>(
        "clock",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Clock",
            "description": "Renders the current local time.",
            "properties": {
                "format": {
                    "type": "string",
                    "description": "chrono strftime format string.",
                    "default": "%H:%M:%S"
                }
            }
        }),
    );

    registry.register_with_schema::<counter_widget::CounterWidget>(
        "counter",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Counter display",
            "description": "Renders a named counter's value, live-updating on counter bus events.",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Counter name (matches the counter.* actions)."
                },
                "label": {
                    "type": "string",
                    "description": "Optional display label. Defaults to `name`."
                }
            },
            "required": ["name"]
        }),
    );

    registry.register_with_schema::<dynamic::DynamicWidget>(
        "dynamic",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Dynamic view tree",
            "description": "Renders a protocol-defined ViewNode tree published under `tree_key`.",
            "properties": {
                "tree_key": {
                    "type": "string",
                    "description": "Key into the shared `view_trees` map."
                },
                "placeholder": {
                    "type": "string",
                    "description": "Text shown while no tree is published.",
                    "default": ""
                }
            },
            "required": ["tree_key"]
        }),
    );

    registry.register_with_schema::<homelab_status::HomelabStatusWidget>(
        "homelab_status",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Homelab status grid",
            "description": "Polls a list of HTTP endpoints and renders them as a vertical column of labelled status rows.",
            "properties": {
                "interval_ms": {
                    "type": "integer",
                    "minimum": 1000,
                    "description": "Poll interval in milliseconds (clamped to a 1s floor).",
                    "default": 5000
                },
                "probes": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": {
                                "type": "string",
                                "description": "Row label."
                            },
                            "url": {
                                "type": "string",
                                "format": "uri",
                                "description": "Probe URL."
                            }
                        },
                        "required": ["label", "url"]
                    }
                }
            },
            "required": ["probes"]
        }),
    );

    registry.register_with_schema::<http_probe::HttpProbeWidget>(
        "http_probe",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "HTTP status badge",
            "description": "Periodic GET against a URL, rendered as a coloured status badge.",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "Probe URL."
                },
                "label": {
                    "type": "string",
                    "description": "Label displayed above the badge.",
                    "default": "probe"
                },
                "interval_ms": {
                    "type": "integer",
                    "minimum": 500,
                    "description": "Poll interval in milliseconds (clamped to a 500ms floor).",
                    "default": 5000
                }
            },
            "required": ["url"]
        }),
    );

    registry.register_with_schema::<image_widget::ImageWidget>(
        "image",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Static image",
            "description": "Displays a static image from disk, optionally wrapped in a card.",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path or path relative to the profile's assets."
                },
                "title": {
                    "type": "string",
                    "description": "Optional card header above the image."
                }
            },
            "required": ["path"]
        }),
    );

    registry.register_with_schema::<log_feed::LogFeedWidget>(
        "log_feed",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Bus log feed",
            "description": "Rolling list of recent bus messages for a subscribed topic.",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Bus topic to subscribe to."
                },
                "max_rows": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "description": "Maximum number of rows retained (clamped to [1, 50]).",
                    "default": 5
                }
            },
            "required": ["topic"]
        }),
    );

    registry.register_with_schema::<notification_toast::NotificationToastWidget>(
        "notification_toast",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Notification toast",
            "description": "Shows the most recent bus event whose topic starts with one of `prefixes`; fades after `dismiss_after_ms`.",
            "properties": {
                "prefixes": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Topic prefixes to match (any-of).",
                    "default": ["action."]
                },
                "dismiss_after_ms": {
                    "type": "integer",
                    "minimum": 200,
                    "description": "Milliseconds of inactivity before the toast clears (clamped to a 200ms floor).",
                    "default": 3000
                }
            }
        }),
    );

    registry.register_with_schema::<now_playing::NowPlayingWidget>(
        "now_playing",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Now playing",
            "description": "Displays current media title + artist + status via playerctl.",
            "properties": {
                "interval_ms": {
                    "type": "integer",
                    "minimum": 500,
                    "description": "Poll interval in milliseconds (clamped to a 500ms floor).",
                    "default": 2000
                }
            }
        }),
    );

    registry.register_with_schema::<plugin_proxy::PluginProxyWidget>(
        "plugin_proxy",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Plugin proxy",
            "description": "Renders declarative content pushed by a plugin via bus messages. Subscribes to `plugin.<pane_id>.widget_set` by default.",
            "properties": {
                "topic_override": {
                    "type": "string",
                    "description": "Custom bus topic to subscribe to instead of the pane default."
                },
                "title": {
                    "type": "string",
                    "description": "Card header (default \"plugin\").",
                    "default": "plugin"
                }
            }
        }),
    );

    registry.register_with_schema::<sysinfo_widget::SysinfoWidget>(
        "sysinfo",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "System info",
            "description": "CPU and memory stats.",
            "properties": {
                "interval_ms": {
                    "type": "integer",
                    "minimum": 200,
                    "description": "Refresh cadence in milliseconds (clamped to a 200ms floor).",
                    "default": 1000
                }
            }
        }),
    );

    registry.register_with_schema::<text::Text>(
        "text",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Static text",
            "description": "Renders a static string at small, body, or heading size. Wrapped in a card when `title` is provided.",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Text to render."
                },
                "size": {
                    "type": "string",
                    "enum": ["small", "body", "heading"],
                    "description": "Rendered text size.",
                    "default": "body"
                },
                "title": {
                    "type": "string",
                    "description": "Optional card header. When present the text is wrapped in a card with this title."
                }
            },
            "required": ["content"]
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_widget_schema_exposes_content() {
        let mut r = WidgetRegistry::new();
        register_builtins(&mut r);
        let schema = r.schema_for("text").expect("text schema");
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["content"].is_object());
        let required = schema["required"].as_array().expect("required array");
        assert!(required.iter().any(|v| v.as_str() == Some("content")));
    }

    #[test]
    fn every_registered_widget_has_a_schema() {
        let mut r = WidgetRegistry::new();
        register_builtins(&mut r);
        for name in r.names() {
            assert!(
                r.schema_for(name).is_some(),
                "missing JSON Schema for widget `{name}`"
            );
        }
    }

    #[test]
    fn text_size_uses_enum() {
        let mut r = WidgetRegistry::new();
        register_builtins(&mut r);
        let schema = r.schema_for("text").expect("text schema");
        let values: Vec<&str> = schema["properties"]["size"]["enum"]
            .as_array()
            .expect("enum array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(values, vec!["small", "body", "heading"]);
    }
}
