use super::*;
use crate::action::ActionRegistry;
use serde_json::json;

/// Register every built-in action along with its JSON Schema (Draft-07).
///
/// The web editor pulls these schemas via `ActionRegistry::schema_for` to auto-generate
/// config forms. When adding a new action, register it with `register_with_schema` and
/// author a schema that mirrors the TOML args the action's `BuildFromArgs` reads.
pub fn register_builtins(registry: &mut ActionRegistry) {
    registry.register_with_schema::<app_launch::AppLaunch>(
        "app.launch",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Launch an application",
            "description": "Spawn an executable in the background, fire-and-forget.",
            "properties": {
                "exe": {
                    "type": "string",
                    "description": "Executable name or absolute path."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Argument vector passed to the executable.",
                    "default": []
                }
            },
            "required": ["exe"]
        }),
    );

    registry.register_with_schema::<clipboard_set::ClipboardSet>(
        "clipboard.set",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Set clipboard text",
            "description": "Write a string to the system clipboard.",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to place on the clipboard."
                }
            },
            "required": ["text"]
        }),
    );

    registry.register_with_schema::<counter_decrement::CounterDecrement>(
        "counter.decrement",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Decrement counter",
            "description": "Subtract from a named counter stored in deck state.",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Counter name (key in state.bindings)."
                },
                "step": {
                    "type": "integer",
                    "description": "Amount to subtract per press.",
                    "default": 1
                }
            },
            "required": ["name"]
        }),
    );

    registry.register_with_schema::<counter_display::CounterDisplay>(
        "counter.display",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Display counter value",
            "description": "Render a counter's current value on the tile label; press is a no-op.",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Counter name (key in state.bindings)."
                }
            },
            "required": ["name"]
        }),
    );

    registry.register_with_schema::<counter_increment::CounterIncrement>(
        "counter.increment",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Increment counter",
            "description": "Add to a named counter stored in deck state.",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Counter name (key in state.bindings)."
                },
                "step": {
                    "type": "integer",
                    "description": "Amount to add per press.",
                    "default": 1
                }
            },
            "required": ["name"]
        }),
    );

    registry.register_with_schema::<deck_hold_cycle::DeckHoldCycle>(
        "deck.hold_cycle",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Hold-to-reverse page cycle",
            "description": "Short press advances to the next page in the list; long press goes back.",
            "properties": {
                "pages": {
                    "type": "array",
                    "items": {"type": "string"},
                    "minItems": 1,
                    "description": "Page names to cycle through."
                },
                "long_press_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Duration in milliseconds that counts as a long press.",
                    "default": 400
                }
            },
            "required": ["pages"]
        }),
    );

    registry.register_with_schema::<deck_page_back::DeckPageBack>(
        "deck.page_back",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Go back one page",
            "description": "Pop the page history stack.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<deck_page_goto::DeckPageGoto>(
        "deck.page_goto",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Go to page",
            "description": "Switch the deck to the named page.",
            "properties": {
                "page": {
                    "type": "string",
                    "description": "Target page name."
                }
            },
            "required": ["page"]
        }),
    );

    registry.register_with_schema::<deck_profile_switch::DeckProfileSwitch>(
        "deck.profile_switch",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Switch profile",
            "description": "Switch the deck to the named profile.",
            "properties": {
                "profile": {
                    "type": "string",
                    "description": "Target profile name."
                }
            },
            "required": ["profile"]
        }),
    );

    registry.register_with_schema::<deck_scroll_down::DeckScrollDown>(
        "deck.scroll_down",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Scroll page down",
            "description": "Shift the visible 4x4 window one row down in the page's logical grid.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<deck_scroll_left::DeckScrollLeft>(
        "deck.scroll_left",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Scroll page left",
            "description": "Shift the visible 4x4 window one column to the left in the page's logical grid.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<deck_scroll_right::DeckScrollRight>(
        "deck.scroll_right",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Scroll page right",
            "description": "Shift the visible 4x4 window one column to the right in the page's logical grid.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<deck_scroll_up::DeckScrollUp>(
        "deck.scroll_up",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Scroll page up",
            "description": "Shift the visible 4x4 window one row up in the page's logical grid.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<grafana_dashboard_open::GrafanaDashboardOpen>(
        "grafana.dashboard_open",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Open Grafana dashboard",
            "description": "Open `{base}/d/{uid}` in the system browser.",
            "properties": {
                "base": {
                    "type": "string",
                    "format": "uri",
                    "description": "Grafana root URL, e.g. http://docker2.lan:3000."
                },
                "uid": {
                    "type": "string",
                    "description": "Dashboard uid."
                }
            },
            "required": ["base", "uid"]
        }),
    );

    registry.register_with_schema::<http_get::HttpGet>(
        "http.get",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "HTTP GET",
            "description": "Fire-and-forget HTTP GET request.",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "Request URL."
                },
                "headers": {
                    "type": "object",
                    "description": "Additional request headers (string → string).",
                    "additionalProperties": {"type": "string"},
                    "default": {}
                }
            },
            "required": ["url"]
        }),
    );

    registry.register_with_schema::<http_post::HttpPost>(
        "http.post",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "HTTP POST",
            "description": "POST with optional JSON or raw body.",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "Request URL."
                },
                "headers": {
                    "type": "object",
                    "description": "Additional request headers (string → string).",
                    "additionalProperties": {"type": "string"},
                    "default": {}
                },
                "json": {
                    "description": "JSON body. Takes precedence over `body`.",
                    "type": ["object", "array", "string", "number", "boolean", "null"]
                },
                "body": {
                    "type": "string",
                    "description": "Raw text body, used if `json` is absent."
                }
            },
            "required": ["url"]
        }),
    );

    registry.register_with_schema::<http_probe::HttpProbe>(
        "http.probe",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "HTTP probe",
            "description": "GET the URL and report whether the status falls inside [ok_min, ok_max).",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "Probe URL."
                },
                "ok_min": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 599,
                    "description": "Inclusive lower bound of the 'ok' status range.",
                    "default": 200
                },
                "ok_max": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 600,
                    "description": "Exclusive upper bound of the 'ok' status range.",
                    "default": 400
                }
            },
            "required": ["url"]
        }),
    );

    registry.register_with_schema::<keypress::Keypress>(
        "keypress",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Send a key chord",
            "description": "Simulate a keyboard chord (comma-separated key names, e.g. \"ctrl,shift,t\").",
            "properties": {
                "keys": {
                    "type": "string",
                    "description": "Comma-separated key names. Modifiers: ctrl/control, alt, shift, meta/super/cmd. Named: tab, enter, esc, space, backspace, delete, up/down/left/right, home, end, pageup, pagedown, f1..f12. Single characters are typed as Unicode."
                }
            },
            "required": ["keys"]
        }),
    );

    registry.register_with_schema::<media_next::MediaNext>(
        "media.next",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Media: next track",
            "description": "Skip to the next track via `playerctl next`.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<media_playpause::MediaPlaypause>(
        "media.playpause",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Media: play/pause",
            "description": "Toggle play/pause via `playerctl play-pause`.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<media_prev::MediaPrev>(
        "media.prev",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Media: previous track",
            "description": "Go to previous track via `playerctl previous`.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<media_vol_down::MediaVolDown>(
        "media.vol_down",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Media: volume down",
            "description": "Lower volume by 5% via `playerctl volume 0.05-`.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<media_vol_up::MediaVolUp>(
        "media.vol_up",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Media: volume up",
            "description": "Raise volume by 5% via `playerctl volume 0.05+`.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<multi_branch::MultiBranch>(
        "multi.branch",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Conditional shell branch",
            "description": "Run `if_cmd`; dispatch `then_cmd` on success, `else_cmd` on failure. Commands run via `sh -c` (unix) or `cmd /C` (windows).",
            "properties": {
                "if_cmd": {
                    "type": "string",
                    "description": "Condition command. Success = exit 0."
                },
                "then_cmd": {
                    "type": "string",
                    "description": "Command to run if `if_cmd` succeeded."
                },
                "else_cmd": {
                    "type": "string",
                    "description": "Command to run if `if_cmd` failed."
                }
            },
            "required": ["if_cmd"]
        }),
    );

    registry.register_with_schema::<multi_delay::MultiDelay>(
        "multi.delay",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Delayed command sequence",
            "description": "Run each step after an optional `delay_ms`. Both fields are optional per step (a step may be a pure sleep or a pure command).",
            "properties": {
                "steps": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "cmd": {
                                "type": "string",
                                "description": "Shell command to execute."
                            },
                            "delay_ms": {
                                "type": "integer",
                                "minimum": 0,
                                "description": "Milliseconds to sleep before running `cmd`."
                            }
                        }
                    }
                }
            },
            "required": ["steps"]
        }),
    );

    registry.register_with_schema::<multi_run_list::MultiRunList>(
        "multi.run_list",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Run shell commands sequentially",
            "description": "Fire a list of shell commands one after another.",
            "properties": {
                "cmds": {
                    "type": "array",
                    "minItems": 1,
                    "items": {"type": "string"},
                    "description": "Commands to run via `sh -c` (unix) or `cmd /C` (windows)."
                }
            },
            "required": ["cmds"]
        }),
    );

    registry.register_with_schema::<ntfy_send::NtfySend>(
        "ntfy.send",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Send ntfy notification",
            "description": "POST a message to an ntfy topic.",
            "properties": {
                "server": {
                    "type": "string",
                    "format": "uri",
                    "description": "ntfy server root URL.",
                    "default": "http://docker2.lan:8555"
                },
                "topic": {
                    "type": "string",
                    "description": "ntfy topic name."
                },
                "message": {
                    "type": "string",
                    "description": "Message body."
                },
                "user": {
                    "type": "string",
                    "description": "Basic-auth username (pair with `pass`)."
                },
                "pass": {
                    "type": "string",
                    "description": "Basic-auth password."
                },
                "priority": {
                    "type": "string",
                    "enum": ["min", "low", "default", "high", "max"],
                    "description": "ntfy Priority header."
                }
            },
            "required": ["topic", "message"]
        }),
    );

    registry.register_with_schema::<open_url::OpenUrl>(
        "open.url",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Open URL",
            "description": "Invoke `xdg-open` (linux) or `start` (windows) on a URL.",
            "properties": {
                "url": {
                    "type": "string",
                    "format": "uri",
                    "description": "URL to open."
                }
            },
            "required": ["url"]
        }),
    );

    registry.register_with_schema::<portainer_stack_restart::PortainerStackRestart>(
        "portainer.stack_restart",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Restart a Portainer stack",
            "description": "POST /stop then /start against a Portainer stack.",
            "properties": {
                "base": {
                    "type": "string",
                    "format": "uri",
                    "description": "Portainer root URL, e.g. https://portainer.jacobsabella.com."
                },
                "stack_id": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Portainer stack id."
                },
                "endpoint_id": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Portainer environment id (e.g. 4 for docker2)."
                },
                "api_key": {
                    "type": "string",
                    "description": "Portainer API token (X-API-Key header value)."
                }
            },
            "required": ["base", "stack_id", "endpoint_id", "api_key"]
        }),
    );

    registry.register_with_schema::<shell_run::ShellRun>(
        "shell.run",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Run shell command",
            "description": "Execute a command line via `sh -c` (unix) or `cmd /C` (windows).",
            "properties": {
                "cmd": {
                    "type": "string",
                    "description": "Command to execute."
                }
            },
            "required": ["cmd"]
        }),
    );

    registry.register_with_schema::<system_brightness::SystemBrightness>(
        "system.brightness",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Set brightness",
            "description": "Adjust the backlight via `brightnessctl set <delta>`.",
            "properties": {
                "delta": {
                    "type": "string",
                    "description": "brightnessctl expression, e.g. \"+5%\", \"-10%\", or \"50%\"."
                }
            },
            "required": ["delta"]
        }),
    );

    registry.register_with_schema::<system_mic_mute::SystemMicMute>(
        "system.mic_mute",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Toggle microphone mute",
            "description": "Toggle the default source mute via pactl.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<system_screenshot::SystemScreenshot>(
        "system.screenshot",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Screenshot (grim)",
            "description": "Save a screenshot via `grim <path>`.",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Filesystem path for the output image.",
                    "default": "/tmp/juballer-screenshot.png"
                }
            }
        }),
    );

    registry.register_with_schema::<text_md_template::TextMdTemplate>(
        "text.md_template",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Type markdown template",
            "description": "Type a markdown snippet (heading, list, or code block).",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["h2", "h3", "list", "code"],
                    "description": "Template kind."
                },
                "text": {
                    "type": "string",
                    "description": "Content placed inside the template.",
                    "default": ""
                }
            },
            "required": ["kind"]
        }),
    );

    registry.register_with_schema::<text_snippet_expand::TextSnippetExpand>(
        "text.snippet_expand",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Expand snippet template",
            "description": "Type a snippet template, substituting current date/time. Supported placeholders: {date}, {time}, {datetime}, {uuid}.",
            "properties": {
                "template": {
                    "type": "string",
                    "description": "Template text with supported placeholders."
                }
            },
            "required": ["template"]
        }),
    );

    registry.register_with_schema::<text_type_string::TextTypeString>(
        "text.type_string",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Type a string",
            "description": "Type a literal string into the focused window.",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to type."
                }
            },
            "required": ["text"]
        }),
    );

    registry.register_with_schema::<tile_set_shader::TileSetShader>(
        "tile.set_shader",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Set tile shader / video",
            "description": "Swap the current tile's raw-wgpu content source. Exactly one of `wgsl` (path to a .wgsl file) or `video` (URI, e.g. v4l2:///dev/video0) must be present.",
            "properties": {
                "wgsl": {
                    "type": "string",
                    "description": "Absolute path to a WGSL fragment shader file."
                },
                "video": {
                    "type": "string",
                    "description": "Video source URI. Currently only v4l2:///dev/videoN is supported."
                }
            },
            "oneOf": [
                {"required": ["wgsl"]},
                {"required": ["video"]}
            ]
        }),
    );

    registry.register_with_schema::<tile_clear_shader::TileClearShader>(
        "tile.clear_shader",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Clear tile shader",
            "description": "Remove the tile's raw-wgpu shader source; the tile renders as plain egui again.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<timer_countdown::TimerCountdown>(
        "timer.countdown",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Countdown timer",
            "description": "Count down `seconds` seconds, updating the tile label and publishing a done event when finished.",
            "properties": {
                "seconds": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Countdown length in seconds."
                },
                "label": {
                    "type": "string",
                    "description": "Tile label prefix.",
                    "default": "timer"
                }
            },
            "required": ["seconds"]
        }),
    );

    registry.register_with_schema::<timer_pomodoro::TimerPomodoro>(
        "timer.pomodoro",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Pomodoro timer",
            "description": "Focus + break cycle. First press starts; press again to cancel mid-cycle.",
            "properties": {
                "focus_min": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Focus duration in minutes.",
                    "default": 25
                },
                "break_min": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Break duration in minutes.",
                    "default": 5
                }
            }
        }),
    );

    registry.register_with_schema::<timer_stopwatch::TimerStopwatch>(
        "timer.stopwatch",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Stopwatch",
            "description": "First press starts the stopwatch; second press stops and reports elapsed time.",
            "properties": {}
        }),
    );

    registry.register_with_schema::<toggle_cycle_n::ToggleCycleN>(
        "toggle.cycle_n",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Cycle N-state toggle",
            "description": "Cycle a named state index through `count` values.",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Toggle name (key in state.bindings)."
                },
                "count": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Number of distinct states."
                },
                "labels": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional per-state labels. If present, the tile label is set to labels[i] on each press.",
                    "default": []
                }
            },
            "required": ["name", "count"]
        }),
    );

    registry.register_with_schema::<toggle_onoff::ToggleOnoff>(
        "toggle.onoff",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "On/off toggle",
            "description": "Flip a boolean state and render on/off label + state color.",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Toggle name (key in state.bindings)."
                },
                "on_label": {
                    "type": "string",
                    "description": "Label shown when the toggle is on.",
                    "default": "on"
                },
                "off_label": {
                    "type": "string",
                    "description": "Label shown when the toggle is off.",
                    "default": "off"
                },
                "on_color": {
                    "type": "array",
                    "items": {"type": "integer", "minimum": 0, "maximum": 255},
                    "minItems": 4,
                    "maxItems": 4,
                    "description": "RGBA color applied to the tile when on.",
                    "default": [35, 165, 90, 255]
                },
                "off_color": {
                    "type": "array",
                    "items": {"type": "integer", "minimum": 0, "maximum": 255},
                    "minItems": 4,
                    "maxItems": 4,
                    "description": "RGBA color applied to the tile when off.",
                    "default": [69, 71, 84, 255]
                }
            },
            "required": ["name"]
        }),
    );

    // plugin_proxy_action is NOT registered statically — it's constructed per plugin manifest,
    // and the plugin manifest declares its own args schema.

    registry.register_with_schema::<rhythm_launch::RhythmLaunch>(
        "rhythm.launch",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Launch rhythm mode",
            "description": "Hand the current process over to juballer's rhythm mode via exec(). \
                Replaces the deck app rather than spawning a child so only one window / HID \
                claim is live at a time.",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "enum": ["play", "calibrate-audio", "tutorial", "settings", "mods"],
                    "description": "Which rhythm subcommand to launch.",
                    "default": "play"
                },
                "chart": {
                    "type": "string",
                    "description": "Path to a .memon file or directory of them (picker). Only used by `play`."
                },
                "difficulty": {
                    "type": "string",
                    "description": "Difficulty key for `play`. Defaults to BSC."
                },
                "audio_offset_ms": {
                    "type": "integer",
                    "description": "--audio-offset-ms flag for `play` / `calibrate-audio` / `tutorial`."
                }
            }
        }),
    );

    registry.register_with_schema::<carla_launch::CarlaLaunch>(
        "carla.launch",
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "title": "Launch carla mode",
            "description": "Hand the current process over to juballer's carla audio-FX control \
                surface mode via exec(). Replaces the deck app rather than spawning a child so \
                only one window / HID claim is live at a time. When `config` is omitted the \
                carla subcommand picks the alphabetically-first file from \
                `~/.config/juballer/carla/configs/`.",
            "properties": {
                "config": {
                    "type": "string",
                    "description": "Path to a Carla configuration TOML."
                }
            }
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_run_schema_exposes_cmd() {
        let mut r = ActionRegistry::new();
        register_builtins(&mut r);
        let schema = r.schema_for("shell.run").expect("shell.run schema");
        assert_eq!(schema["type"], "object");
        assert!(
            schema["properties"]["cmd"].is_object(),
            "shell.run schema must describe `cmd`: {schema}"
        );
        let required = schema["required"].as_array().expect("required array");
        assert!(required.iter().any(|v| v.as_str() == Some("cmd")));
    }

    #[test]
    fn every_registered_action_has_a_schema() {
        let mut r = ActionRegistry::new();
        register_builtins(&mut r);
        for name in r.names() {
            assert!(
                r.schema_for(name).is_some(),
                "missing JSON Schema for action `{name}`"
            );
        }
    }

    #[test]
    fn ntfy_priority_uses_enum() {
        let mut r = ActionRegistry::new();
        register_builtins(&mut r);
        let schema = r.schema_for("ntfy.send").expect("ntfy.send schema");
        let prio = &schema["properties"]["priority"]["enum"];
        let values: Vec<&str> = prio
            .as_array()
            .expect("enum array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(values, vec!["min", "low", "default", "high", "max"]);
    }
}
