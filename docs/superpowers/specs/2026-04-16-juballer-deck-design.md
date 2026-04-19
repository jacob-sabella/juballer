# juballer-deck — design spec

**Date:** 2026-04-16
**Status:** Approved (brainstorming complete)
**Scope:** The Stream-Deck-style application built on top of `juballer-core` v0.1. Adds action/widget engine, Python plugin host, config hierarchy, and a web-based config editor.

## Purpose

Build a live control deck that turns the GAMO2 FB9 controller + calibrated monitor (via `juballer-core`) into a full Stream-Deck-class tool:

1. Each of the 16 grid cells binds to an **Action** with full lifecycle (on_will_appear / on_down / on_up / on_will_disappear) and can push live icon/label updates.
2. The top region hosts **Widgets** (clock, log feed, http probe, etc.) that render via the `juballer-egui` overlay and react to events.
3. Users can extend both actions and widgets with **Python plugins** that run as separate processes and communicate over a UDS (cross-platform via the `interprocess` crate) using line-delimited JSON.
4. Config lives in a directory tree of TOML files under `~/.config/juballer/deck/`, hot-reloaded via `notify`.
5. A **web-based config editor** (axum + vanilla TS SPA bundled into the binary) runs at `localhost:7373` and speaks REST + WebSocket for live editing.
6. Deck ships with a large **built-in action + widget catalog** across 5 categories so out-of-the-box coverage is broad; plugins extend beyond.

The deck does NOT ship its own scripting interpreter. It uses the user's `python3`.

## Target hardware and platforms

- Uses `juballer-core` targets: Linux (primary) + Windows. No macOS in v1.
- Controller: GAMO2 FB9 (HID keyboard mode), identified by profile's `controller_vid_pid`.
- Display: any monitor resolvable by `AppBuilder::on_monitor(desc)`.

## Workspace additions

```
juballer/ (cargo workspace — existing)
├── crates/
│   ├── juballer-core/                unchanged
│   ├── juballer-egui/                unchanged
│   ├── juballer-gestures/            unchanged
│   ├── juballer-deck-protocol/       NEW: wire format types
│   │   └── src/lib.rs                serde + serde_json only, no heavy deps
│   └── juballer-deck/                NEW: the application binary
│       ├── src/main.rs
│       ├── src/app.rs                shell; wires juballer-core
│       ├── src/action/               Action trait + registry + built-ins (~40 actions)
│       ├── src/widget/               Widget trait + registry + built-ins (~12 widgets)
│       ├── src/plugin/               host: spawn, supervise, UDS listener
│       ├── src/config/               schema, load, watch, hot-reload
│       ├── src/bus/                  intra-process event bus (tokio broadcast)
│       ├── src/editor/               axum server + bundled TS SPA
│       └── src/render.rs             glue: profile → juballer-core App
└── deck-py-sdk/                      Python package (PyPI-publishable)
    ├── pyproject.toml
    └── src/juballer_deck/            Action/Widget base classes + asyncio UDS client
```

Two new crates (`juballer-deck-protocol` + `juballer-deck`) and one Python package (`juballer-deck-py-sdk`). Core/egui/gestures are untouched.

## Config

**Location:**
- Linux: `${XDG_CONFIG_HOME:-~/.config}/juballer/deck/`
- Windows: `%APPDATA%\juballer\deck\`

**Tree:**

```
deck/
├── deck.toml                         global settings
├── profiles/
│   └── <name>/
│       ├── profile.toml              profile metadata + page list + env vars
│       ├── pages/<page>.toml         one page per file: layout tree + widget bindings + 16 buttons
│       └── assets/icons/ …           profile-owned icons (svg/png)
├── plugins/
│   └── <name>/
│       ├── manifest.toml             name, entry_point, language, declared actions/widgets
│       ├── plugin.py                 (if language == "python")
│       └── icons/ …
└── state.toml                        persisted counters/toggles/last-active-page
```

**`deck.toml`:**

```toml
version = 1
active_profile = "homelab"

[editor]
bind = "127.0.0.1:7373"
require_auth = false

[render]
monitor_desc = "AOC 2770G4"
present_mode = "fifo"
bg = "#0b0d12"

[log]
level = "info"
```

**`profiles/<name>/profile.toml`:**

```toml
name = "homelab"
description = "Homelab control deck"
default_page = "home"
pages = ["home", "media"]

[env]                                 # profile-scoped vars; referenced as $var
grafana_base = "http://docker2.lan:3000"
ntfy_topic   = "rocket-league"
```

**`profiles/<name>/pages/<page>.toml`:**

```toml
[meta]
title = "home"

# top region layout tree
[[top]]
kind = "stack"
dir  = "vertical"
gap  = 10
children = [
    { size = { fixed = 48 }, pane = "header" },
    { size = { ratio = 1.0 }, stack = { dir = "horizontal", gap = 10, children = [
        { size = { ratio = 1.2 }, pane = "focus" },
        { size = { ratio = 1.0 }, pane = "events" },
        { size = { ratio = 0.7 }, pane = "pages" },
    ]}},
]

# widgets bound to panes
[top.pane.header]
widget = "clock"
format = "%H:%M  %a %d %b"

[top.pane.focus]
widget = "http_probe"
url = "$grafana_base/api/health"
label = "grafana"
interval_ms = 5000

[top.pane.events]
widget = "log_feed"
source = { ntfy_topic = "$ntfy_topic" }
max_rows = 5

[top.pane.pages]
widget = "action_mini"
action = "deck.page_switcher"

# 16 grid buttons
[[button]]
row = 0
col = 0
action = "media.playpause"
icon = "▶"
label = "play"

# ... 15 more
```

**`plugins/<name>/manifest.toml`:**

```toml
name = "discord"
version = "0.1.0"
entry_point = "plugin.py"
language = "python"
actions = ["discord.mute", "discord.deafen", "discord.channel_join", "discord.send_message"]
widgets = ["discord_status", "discord_mentions"]
```

**Variable interpolation:**
- `$var` — profile env lookup
- `$ENV_VAR` — process env
- `${FOO:-default}` — default fallback
- Applied at action-dispatch time, not load time.

**Hot reload:** `notify` watcher on the full tree, 300 ms debounce, delta diffing:
- Profile-wide change → re-init render + action/widget registries for active profile
- Page change → rebuild layout + rebind actions/widgets for that page
- Plugin manifest change → restart that plugin's process
- Parse error → toast widget + keep last-good config

**`state.toml`** serialized on shutdown + every 30 s; merged back into action/widget instances on `on_will_appear`.

## Action + Widget trait model

**Action:**

```rust
pub trait Action: Send + 'static {
    fn on_will_appear(&mut self, cx: &mut ActionCx) { let _ = cx; }
    fn on_down(&mut self, cx: &mut ActionCx) { let _ = cx; }
    fn on_up(&mut self, cx: &mut ActionCx) { let _ = cx; }
    fn on_will_disappear(&mut self, cx: &mut ActionCx) { let _ = cx; }
}

pub struct ActionCx<'a> {
    pub cell: (u8, u8),
    pub tile: &'a mut TileHandle,
    pub env: &'a Env,
    pub bus: &'a EventBus,
    pub state: &'a mut StateStore,
    pub rt: &'a tokio::runtime::Handle,
    pub plugin: Option<&'a PluginHandle>,
}

pub enum IconRef {
    Path(PathBuf),      // relative to profile assets/ or absolute
    Emoji(String),      // e.g. "▶", "🎙"
    Builtin(&'static str),  // named icons baked into binary
}

impl TileHandle {
    pub fn set_icon(&mut self, icon: IconRef);
    pub fn set_label(&mut self, text: impl Into<String>);
    pub fn set_bg(&mut self, color: Color);
    pub fn set_state_color(&mut self, color: Color);
    pub fn flash(&mut self, ms: u16);
}
```

**Widget:**

```rust
pub trait Widget: Send + 'static {
    fn on_will_appear(&mut self, cx: &mut WidgetCx) { let _ = cx; }
    fn on_will_disappear(&mut self, cx: &mut WidgetCx) { let _ = cx; }
    /// Return true to request immediate redraw (animations).
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx) -> bool;
}

pub struct WidgetCx<'a> { /* like ActionCx sans tile + cell, with pane: PaneId */ }
```

**Registry pattern** — string name → factory that consumes a `toml::Value`:

```rust
pub struct ActionRegistry {
    factories: HashMap<&'static str, Box<dyn Fn(&toml::Value) -> Result<Box<dyn Action>>>>,
}
impl ActionRegistry {
    pub fn register<A: Action + BuildFromArgs>(&mut self, name: &'static str);
    pub fn build(&self, name: &str, args: &toml::Value) -> Result<Box<dyn Action>>;
}
pub trait BuildFromArgs: Sized {
    fn from_args(args: &toml::Value) -> Result<Self>;
}
```

Same pattern for `WidgetRegistry`.

**Plugin-proxy** is one more built-in registered per plugin-declared name:

```rust
for name in plugin_manifest.actions {
    registry.register_proxy_action(name, plugin_handle.clone());
}
```

A proxy action's `on_down` etc. marshals JSON to the plugin via UDS and applies returned tile updates.

**Async side-effects** use `cx.rt.spawn` + publish to `bus`; render thread consumes at frame boundary. No `Arc<Mutex<TileHandle>>` leaks to async tasks.

## Built-in action catalog (v1)

| Category | Actions |
|----------|---------|
| **Core OS** | `shell.run`, `keypress`, `clipboard.set`, `app.launch`, `open.url` |
| **Media** | `media.playpause`, `media.next`, `media.prev`, `media.vol_up`, `media.vol_down` |
| **Deck navigation** | `deck.page_goto`, `deck.page_back`, `deck.profile_switch`, `deck.hold_cycle` |
| **HTTP** | `http.get`, `http.post`, `http.form`, `http.probe` (with JSONPath check) |
| **Homelab** | `ntfy.send`, `mqtt.publish`, `mqtt.subscribe_display`, `portainer.stack_restart`, `portainer.container_logs_tail`, `portainer.health_probe`, `n8n.workflow_run`, `n8n.workflow_status`, `grafana.dashboard_open` |
| **Streaming** | `obs.scene_switch`, `obs.source_toggle`, `obs.stream_start`, `obs.stream_stop`, `obs.record_toggle`, `obs.replay_buffer_save`, `discord.mute`, `discord.deafen`, `discord.channel_join`, `discord.react`, `twitch.chat_send`, `twitch.category_set`, `twitch.marker` |
| **Utility** | `text.type_string`, `text.snippet_expand`, `text.md_template`, `timer.countdown`, `timer.stopwatch`, `timer.pomodoro`, `counter.increment`, `counter.decrement`, `counter.display`, `toggle.onoff`, `toggle.cycle_n`, `multi.run_list`, `multi.delay`, `multi.branch` |
| **Smart Home** | `homeassistant.service_call`, `homeassistant.entity_toggle`, `homeassistant.state_as_icon`, `hue.light_on`, `hue.light_off`, `hue.light_color`, `hue.light_scene`, `system.volume`, `system.brightness`, `system.mic_mute`, `system.screenshot` |

Built-in widget catalog (v1): `clock`, `sysinfo`, `now_playing`, `log_feed`, `http_probe`, `homelab_status`, `notification_toast`, `text`, `image`, `counter`, `action_mini`, `plugin_proxy`.

Total: ~47 actions + 12 widgets in `juballer-deck::{action, widget}` modules.

## Plugin protocol (UDS + NDJSON)

**Transport:**
- Linux: UDS at `${XDG_RUNTIME_DIR:-/tmp}/juballer/plugins/<plugin-name>.sock`
- Windows: named pipe at `\\.\pipe\juballer-<plugin-name>`
- Via `interprocess` crate on both sides; same API.
- **Framing:** newline-delimited JSON.

**Spawn sequence:**
1. Deck reads `plugins/<name>/manifest.toml`.
2. Deck creates socket/pipe.
3. Deck spawns plugin via `manifest.entry_point` with env `JUBALLER_SOCK=…`, `JUBALLER_PLUGIN_NAME=…`, `JUBALLER_PROTOCOL_VERSION=1`.
4. Plugin connects.
5. Handshake — both sides exchange `hello`:

```json
// deck → plugin
{"type":"hello","v":1,"deck_version":"0.1.0"}
// plugin → deck
{"type":"hello","v":1,"plugin":"discord","plugin_version":"0.1.0","sdk":"py-0.1.0"}
```

6. Deck sends `register_complete`.

**Deck → plugin messages:**

`binding_id` format: `"<page_name>:<row>,<col>"` (e.g. `"home:2,0"`). For widget bindings, deck uses `pane_id` instead (the PaneId string from the layout tree).

```json
{"type":"will_appear","action":"discord.mute","binding_id":"home:2,0","args":{…}}
{"type":"will_disappear","binding_id":"home:2,0"}
{"type":"key_down","binding_id":"home:2,0"}
{"type":"key_up","binding_id":"home:2,0"}
{"type":"widget_will_appear","widget":"discord_status","pane_id":"focus","args":{…}}
{"type":"widget_will_disappear","pane_id":"focus"}
{"type":"event","topic":"…","data":{…}}
```

**Plugin → deck messages:**

```json
{"type":"tile_set","binding_id":"home:2,0","icon":"icons/mic_off.svg","label":"muted","state_color":"#f38ba8"}
{"type":"tile_flash","binding_id":"home:2,0","ms":200}
{"type":"widget_set","pane_id":"focus","content":{…}}   // declarative widget schema, see below
{"type":"bus_publish","topic":"…","data":{…}}
{"type":"bus_subscribe","topics":["…"]}
{"type":"log","level":"info","msg":"…"}
{"type":"error","code":"…","msg":"…"}
{"type":"pong"}
```

**Declarative widget schema (plugins can't paint egui directly):**

```json
{"type":"widget_set","pane_id":"focus","content":{
  "layout":"vertical","gap":6,"children":[
    {"heading":"Karmine Corp vs G2"},
    {"label":"Game 3 · BO7 · 1-1"},
    {"spacer":true},
    {"big":"2 — 1","small":"live · 02:14"}
  ]
}}
```

v1 layout primitives: `vertical`, `horizontal`. v1 content primitives: `heading`, `label`, `big`, `small`, `spacer`, `image`, `badge`. Deck's `plugin_proxy` widget decodes + drives egui::Ui.

**Supervision:**
- Heartbeat `{"type":"ping"}` every 5 s; plugin must reply within 3 s.
- Crash / timeout → reap + respawn with backoff 1 s → 2 s → 4 s → max 30 s.
- While dead, actions from that plugin render with a `!` badge.
- Malformed message → log + drop; don't kill plugin.
- Plugin unhandled exception (SDK side) → caught, logged via `log` message, action instance reset.

**Versioning:** `v` field in `hello` is integer. Incompatible bumps reject connection with a human-readable upgrade message.

## Python SDK (`juballer-deck-py-sdk`)

User-facing API:

```python
from juballer_deck import Plugin, Action, Widget

plugin = Plugin("discord")

@plugin.action("discord.mute")
class MuteAction(Action):
    def on_will_appear(self, ctx):
        self.client = discord_ipc_connect()
        ctx.tile_set(icon="mic.svg", label="unmuted")
    def on_down(self, ctx):
        muted = not self.client.is_muted()
        self.client.set_muted(muted)
        ctx.tile_set(
            icon="mic_off.svg" if muted else "mic.svg",
            label="muted" if muted else "unmuted",
            state_color="#f38ba8" if muted else "#5865f2",
        )
    def on_will_disappear(self, ctx):
        self.client.close()

@plugin.widget("discord_status")
class StatusWidget(Widget):
    def render(self, ctx):
        state = ctx.get("discord.voice.state", {})
        return ctx.vbox(
            ctx.heading(state.get("channel", "—")),
            ctx.label(f"{len(state.get('members', []))} in voice"),
        )

if __name__ == "__main__":
    plugin.run()
```

Internals: `plugin.run()` connects to `JUBALLER_SOCK`, runs an `asyncio` event loop, dispatches incoming NDJSON messages to decorated classes. Each binding gets one instance. Exceptions in user code are caught and reported via `log` messages.

Published to PyPI alongside a CLI entry point `juballer-plugin run <path>` that wraps `plugin.run()` for local dev.

## Web config editor

**Stack:**
- Server: `axum` (sharing the deck's tokio runtime).
- Client: vanilla TypeScript + Vite. Bundle < 500 KB gzipped.
- Bundle is `include_bytes!()`-ed into the deck binary.
- Bind `127.0.0.1:7373` by default. LAN access via opt-in `[editor].bind = "0.0.0.0:7373"` + `require_auth = true` (token rotates on deck restart).

**REST API under `/api/v1`:**

```
GET    /state
GET    /profiles
GET    /profiles/:p
POST   /profiles/:p
DELETE /profiles/:p
POST   /profiles/:p/activate
GET    /profiles/:p/pages/:page
POST   /profiles/:p/pages/:page
GET    /actions
GET    /widgets
GET    /plugins
POST   /plugins/:name/restart
GET    /assets/*path
POST   /assets/*path
GET    /env
```

**WebSocket `/ws`:**

Deck → editor: `profile_reloaded`, `plugin_status`, `key_preview` (live pushthrough of physical button presses during editing).
Editor → deck: `preview_action` (try-before-save — fires an action without writing config).

**UI views:**
1. **Grid editor** — 4×4 layout, click cell → right panel with action picker (searchable from `/actions`), args form auto-generated from each action's JSON Schema, icon picker (browse `assets/`, upload, emoji), label text, live preview of the deck's render.
2. **Top-region editor** — recursive Stack/Pane tree editor with sliders for sizing. Drag-drop reordering.
3. **Plugins view** — table of plugins (status: ok/crashed/restarting), restart buttons, live stderr log streamed via WS, manifest editor.

**Write flow:**

```
editor POST → atomic write (tempfile + rename) → notify watcher (300 ms debounce)
  → config loader parses + diffs → render layer applies delta
  → WS broadcasts profile_reloaded → editor refetches + renders
```

Filesystem is source of truth. Round-trip typically < 50 ms on localhost.

**Schema exposure:** each action/widget registers a JSON Schema for its args. Editor auto-generates forms. Adding a new action = form appears automatically.

## Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")] Config(String),
    #[error("config io: {0}")] ConfigIo(#[from] std::io::Error),
    #[error("config parse: {path}: {source}")] ConfigParse { path: PathBuf, source: toml::de::Error },
    #[error("plugin {name}: {msg}")] Plugin { name: String, msg: String },
    #[error("editor server: {0}")] Editor(String),
    #[error("action registry: unknown action {0}")] UnknownAction(String),
    #[error("widget registry: unknown widget {0}")] UnknownWidget(String),
    #[error("ipc protocol: {0}")] Protocol(#[from] serde_json::Error),
    #[error("core: {0}")] Core(#[from] juballer_core::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
```

**Error surfacing policy:**
- Action runtime errors → logged + `tile.flash(red)`, action instance survives.
- Plugin crash → auto-respawn, bindings keep their positions but show `!` badge until reconnected.
- Config parse error → toast widget, keep last-good config.
- No action or plugin fault should kill the deck.

## Testing strategy

**1. Unit tests per module.** Built-in actions test their effect on a mock `ActionCx`. Widgets test render against `egui_kittest`. Config schema round-trips against golden TOMLs in `tests/fixtures/`.

**2. Protocol tests** in `juballer-deck-protocol` — every message type serializes to JSON and back, validates against schema. No network.

**3. Plugin integration tests** — spawn a tiny test plugin binary (written in Rust, not Python, to keep CI dependency-free), run the deck's plugin host against it, assert lifecycle (`hello` → `will_appear` → `key_down` → `tile_set` → `will_disappear`).

**4. End-to-end editor tests** — spin axum server in-process against an in-memory profile, exercise endpoints with `reqwest`, assert state transitions.

Hardware-in-the-loop tests are manual: run `juballer-deck --once` with a real profile, press FB9 buttons, eyeball.

## Performance contract

- Button press → first side effect:
  - Built-in action: **≤ 2 ms** p99
  - Plugin action: **≤ 5 ms** p99 (UDS round-trip + plugin handler)
- Widget update → on-screen pixel: **≤ 1 frame** (egui immediate mode → next composite).
- Editor POST → hot reload applied → WS push: **≤ 100 ms** p99 local.
- Memory: idle deck with 16 built-ins + 4 plugins loaded → target **< 80 MB RSS** (Rust process) + ~30 MB per Python plugin.
- No sandboxing beyond process isolation in v1.

## Binary CLI

```
juballer-deck                     # run with default config dir
juballer-deck --config <dir>
juballer-deck --profile <name>
juballer-deck --monitor <desc>
juballer-deck --once              # render one frame + exit (smoke)
juballer-deck --debug             # enable debug overlay (juballer-core)
juballer-deck check               # validate config, don't run
juballer-deck plugin run <path>   # run a plugin standalone, stdin-attached, for dev
juballer-deck plugin reload <name>
juballer-deck profile list
juballer-deck profile switch <name>
juballer-deck editor open         # xdg-open http://localhost:7373
```

## Out of scope for v1

- Multi-display layouts (controller on display A, top region on display B)
- Cloud profile sync
- Action undo/redo in editor
- Plugin sandboxing (cgroup/seccomp/etc.)
- Gesture recognizer integration (juballer-core has it; deck will consume later when a specific shortcut is needed)
- macOS (blocked on juballer-core targets)
- Non-Python plugin SDKs (Node/Go/Rust plugin-SDK crates) — protocol is language-agnostic; additional SDKs are trivial future work

## Open items resolved during brainstorming

| Question | Resolution |
|----------|------------|
| MVP scope | Full Stream-Deck clone (multi-page, plugins, editor) |
| Action lifecycle | Full: on_will_appear / on_down / on_up / on_will_disappear + push-to-tile |
| Scripting integration | Persistent Python plugin processes via UDS, not embedded PyO3 |
| Config authoring | Web-based editor served by the deck, filesystem remains source of truth |
| Action catalog scope | All 5 categories (Core + Media + Homelab + Streaming + Utility + Smart Home) = ~47 actions |
| Top-region widget model | Mirrors action model; `Widget` trait with on_will_appear / on_will_disappear / render |
| Plugin transport | UDS (Linux) / Named Pipe (Windows) via `interprocess` crate, NDJSON framing |
| Crate structure | 2-crate workspace addition (`juballer-deck` + `juballer-deck-protocol`) + separate PyPI package |
