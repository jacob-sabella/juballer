# juballer-deck Plan A: Foundation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the foundation of the `juballer-deck` binary so that it opens a window on the calibrated monitor, reads a TOML profile from disk, renders its 16 grid cells + top region, dispatches button presses to registered actions, and hot-reloads on file changes. Ships with one smoke-test action (`shell.run`) and two widgets (`clock`, `text`) — just enough for end-to-end validation.

**Architecture:** New workspace members `juballer-deck-protocol` (wire-format types, kept minimal for Plan A — extended in Plan D) and `juballer-deck` (the binary). The binary wires `juballer-core`'s `App` + `juballer-egui`'s `EguiOverlay` together, wraps them with an action/widget registry, layered on top of a config loader that watches the filesystem.

**Tech Stack:** Rust 2021, Tokio (current-thread + spawn), `thiserror`, `serde` + `toml`, `notify` 6 (file watching), `clap` 4 (CLI), `indexmap`, `tracing` (structured logging), `juballer-core` + `juballer-egui` + `juballer-gestures` (companions).

---

## Plan Conventions

- Each task ends with a commit. Conventional Commits: `feat:`, `test:`, `chore:`, `fix:`.
- TDD where unit is pure logic. Smoke-tested via a fixture profile at the end.
- Run `cargo fmt --all` + `cargo clippy --workspace --all-targets -- -D warnings` before each commit.
- The `juballer-deck` crate inherits `#![forbid(unsafe_op_in_unsafe_fn)]` from day 1.
- All public types live in `pub mod` re-exported from `juballer_deck/src/lib.rs` (plus a thin `main.rs`).

---

## Phase 0 — Workspace additions

### Task A0.1: Add `juballer-deck-protocol` crate (skeleton)

**Files:**
- Modify: `Cargo.toml` (workspace `members`)
- Create: `crates/juballer-deck-protocol/Cargo.toml`
- Create: `crates/juballer-deck-protocol/src/lib.rs`

- [ ] **Step 1: Add to workspace members**

Modify `Cargo.toml`, the `members` array becomes:
```toml
members = [
    "crates/juballer-core",
    "crates/juballer-egui",
    "crates/juballer-gestures",
    "crates/juballer-deck-protocol",
    "crates/juballer-deck",
]
```

Also add `tokio`, `notify`, `clap`, `tracing`, `tracing-subscriber` to `[workspace.dependencies]`:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
notify = "6"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde_json = "1"
```

- [ ] **Step 2: Write `crates/juballer-deck-protocol/Cargo.toml`**

```toml
[package]
name = "juballer-deck-protocol"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
description = "Wire-format types for the juballer-deck plugin protocol (UDS + NDJSON)."

[dependencies]
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 3: Write the empty lib.rs**

`crates/juballer-deck-protocol/src/lib.rs`:
```rust
//! Wire-format types for juballer-deck ↔ plugin IPC.
//!
//! Plan A ships a minimal placeholder; Plan D fills in the full message enum.
#![forbid(unsafe_op_in_unsafe_fn)]

/// Protocol version. Plan A reserves v1; Plan D ships the real `Message` enum at this version.
pub const PROTOCOL_VERSION: u32 = 1;
```

- [ ] **Step 4: Verify workspace builds**

```
cargo build --workspace
```

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/juballer-deck-protocol/
git commit -m "chore(workspace): add juballer-deck-protocol skeleton crate"
```

### Task A0.2: Add `juballer-deck` binary crate (skeleton)

**Files:**
- Create: `crates/juballer-deck/Cargo.toml`
- Create: `crates/juballer-deck/src/main.rs`
- Create: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `crates/juballer-deck/Cargo.toml`**

```toml
[package]
name = "juballer-deck"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
description = "Stream-Deck-style application built on juballer-core."

[dependencies]
juballer-core = { path = "../juballer-core" }
juballer-egui = { path = "../juballer-egui" }
juballer-deck-protocol = { path = "../juballer-deck-protocol" }
egui.workspace = true

serde.workspace = true
toml.workspace = true
serde_json.workspace = true
thiserror.workspace = true
indexmap.workspace = true
tokio.workspace = true
notify.workspace = true
clap.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
log.workspace = true

[dev-dependencies]
tempfile = "3"

[[bin]]
name = "juballer-deck"
path = "src/main.rs"
```

- [ ] **Step 2: Write `src/lib.rs`**

```rust
//! juballer-deck — Stream-Deck-style application built on juballer-core.
#![forbid(unsafe_op_in_unsafe_fn)]
```

- [ ] **Step 3: Write `src/main.rs`**

```rust
fn main() {
    println!("juballer-deck: stub — Plan A in progress");
}
```

- [ ] **Step 4: Verify**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/
git commit -m "chore(deck): add juballer-deck binary crate skeleton"
```

---

## Phase 1 — Error type

### Task A1.1: Error enum

**Files:**
- Create: `crates/juballer-deck/src/error.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `error.rs`**

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),

    #[error("config io: {0}")]
    ConfigIo(#[from] std::io::Error),

    #[error("config parse: {path}: {source}")]
    ConfigParse { path: PathBuf, source: toml::de::Error },

    #[error("action registry: unknown action {0}")]
    UnknownAction(String),

    #[error("widget registry: unknown widget {0}")]
    UnknownWidget(String),

    #[error("core: {0}")]
    Core(#[from] juballer_core::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 2: Re-export from lib.rs**

Modify `crates/juballer-deck/src/lib.rs`:
```rust
//! juballer-deck — Stream-Deck-style application built on juballer-core.
#![forbid(unsafe_op_in_unsafe_fn)]

mod error;
pub use error::{Error, Result};
```

- [ ] **Step 3: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): Error enum + Result alias for deck crate"
```

---

## Phase 2 — Config schema

### Task A2.1: Define serde types for the full config tree

**Files:**
- Create: `crates/juballer-deck/src/config/mod.rs`
- Create: `crates/juballer-deck/src/config/schema.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `schema.rs`**

```rust
//! TOML-backed config schema. Types mirror the spec (2026-04-16-juballer-deck-design.md).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

// ---- deck.toml (global) ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckConfig {
    pub version: u32,
    pub active_profile: String,
    #[serde(default)]
    pub editor: EditorConfig,
    #[serde(default)]
    pub render: RenderConfig,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorConfig {
    pub bind: String,
    #[serde(default)]
    pub require_auth: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self { bind: "127.0.0.1:7373".into(), require_auth: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RenderConfig {
    #[serde(default)]
    pub monitor_desc: Option<String>,
    #[serde(default)]
    pub present_mode: Option<String>,
    #[serde(default)]
    pub bg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogConfig {
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self { Self { level: "info".into() } }
}

// ---- profiles/<name>/profile.toml ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub default_page: String,
    pub pages: Vec<String>,
    #[serde(default)]
    pub env: IndexMap<String, String>,
}

// ---- profiles/<name>/pages/<page>.toml ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageConfig {
    pub meta: PageMeta,
    #[serde(default)]
    pub top: Option<LayoutNodeCfg>,
    /// pane id -> widget binding
    #[serde(default)]
    pub top_panes: IndexMap<String, WidgetBindingCfg>,
    #[serde(default, rename = "button")]
    pub buttons: Vec<ButtonCfg>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageMeta {
    pub title: String,
}

/// Layout tree node, TOML-friendly. Serializes to the juballer_core::layout::Node later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayoutNodeCfg {
    Stack {
        kind: String, // always "stack"; kept for readability
        dir: String,  // "horizontal" | "vertical"
        #[serde(default)]
        gap: u16,
        children: Vec<LayoutChildCfg>,
    },
    Pane { pane: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutChildCfg {
    pub size: SizingCfg,
    #[serde(flatten)]
    pub node: LayoutChildNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayoutChildNode {
    Pane { pane: String },
    Stack { stack: StackInner },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackInner {
    pub dir: String,
    #[serde(default)]
    pub gap: u16,
    pub children: Vec<LayoutChildCfg>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SizingCfg {
    Fixed { fixed: u16 },
    Ratio { ratio: f32 },
    Auto { auto: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetBindingCfg {
    pub widget: String,
    #[serde(flatten, default)]
    pub args: toml::Table,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ButtonCfg {
    pub row: u8,
    pub col: u8,
    pub action: String,
    #[serde(default)]
    pub args: toml::Table,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

// ---- state.toml ----

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StateFile {
    #[serde(default)]
    pub last_active_page: Option<String>,
    /// binding_id -> JSON blob (counters, toggles)
    #[serde(default)]
    pub bindings: IndexMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_toml_roundtrip() {
        let s = r#"
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
"#;
        let c: DeckConfig = toml::from_str(s).unwrap();
        let back = toml::to_string(&c).unwrap();
        let c2: DeckConfig = toml::from_str(&back).unwrap();
        assert_eq!(c, c2);
        assert_eq!(c.active_profile, "homelab");
        assert_eq!(c.render.monitor_desc.as_deref(), Some("AOC 2770G4"));
    }

    #[test]
    fn profile_meta_roundtrip() {
        let s = r#"
name = "homelab"
description = "Homelab control deck"
default_page = "home"
pages = ["home", "media"]

[env]
grafana_base = "http://docker2.lan:3000"
ntfy_topic = "rocket-league"
"#;
        let p: ProfileMeta = toml::from_str(s).unwrap();
        let back = toml::to_string(&p).unwrap();
        let p2: ProfileMeta = toml::from_str(&back).unwrap();
        assert_eq!(p, p2);
        assert_eq!(p.env["grafana_base"], "http://docker2.lan:3000");
    }

    #[test]
    fn page_with_buttons_parses() {
        let s = r#"
[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "media.playpause"
icon = "▶"
label = "play"

[[button]]
row = 0
col = 1
action = "shell.run"
args = { cmd = "notify-send hi" }
icon = "🔔"
label = "ping"
"#;
        let p: PageConfig = toml::from_str(s).unwrap();
        assert_eq!(p.buttons.len(), 2);
        assert_eq!(p.buttons[0].action, "media.playpause");
        assert_eq!(p.buttons[1].args.get("cmd").unwrap().as_str().unwrap(), "notify-send hi");
    }

    #[test]
    fn state_file_roundtrip() {
        let mut s = StateFile::default();
        s.last_active_page = Some("home".into());
        s.bindings.insert("home:0,0".into(), serde_json::json!({ "count": 5 }));
        let toml_str = toml::to_string(&s).unwrap();
        let back: StateFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(s, back);
    }
}
```

- [ ] **Step 2: Write `config/mod.rs`**

```rust
//! Config: schema, paths, loading, hot reload.

pub mod schema;
pub use schema::*;
```

- [ ] **Step 3: Wire into lib.rs**

Modify `crates/juballer-deck/src/lib.rs`:
```rust
//! juballer-deck — Stream-Deck-style application built on juballer-core.
#![forbid(unsafe_op_in_unsafe_fn)]

mod error;
pub mod config;
pub use error::{Error, Result};
```

- [ ] **Step 4: Run tests**

```
cargo test -p juballer-deck config::schema::tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expect 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): TOML config schema types with round-trip tests"
```

### Task A2.2: Variable interpolation

**Files:**
- Create: `crates/juballer-deck/src/config/interpolate.rs`
- Modify: `crates/juballer-deck/src/config/mod.rs`

- [ ] **Step 1: Write `interpolate.rs`**

```rust
//! Shell-style variable interpolation for config values.
//! Supports: `$var`, `${var}`, `${var:-default}`.
//! Source of variables: merged (profile.env, process env). Profile env wins.

use std::collections::HashMap;

pub fn interpolate(s: &str, env: &HashMap<String, String>) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b != b'$' {
            out.push(b as char);
            i += 1;
            continue;
        }
        // $
        if i + 1 >= bytes.len() {
            out.push('$');
            break;
        }
        let next = bytes[i + 1];
        if next == b'{' {
            // ${name} or ${name:-default}
            let end = (i + 2..).find(|&j| j < bytes.len() && bytes[j] == b'}');
            if let Some(end) = end {
                let inner = &s[i + 2..end];
                let (name, default) = match inner.find(":-") {
                    Some(p) => (&inner[..p], Some(&inner[p + 2..])),
                    None => (inner, None),
                };
                let v = env
                    .get(name)
                    .cloned()
                    .or_else(|| default.map(|d| d.to_string()))
                    .unwrap_or_default();
                out.push_str(&v);
                i = end + 1;
            } else {
                // unterminated — copy literally
                out.push_str(&s[i..]);
                break;
            }
        } else if next.is_ascii_alphabetic() || next == b'_' {
            // $name — greedy identifier
            let mut end = i + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            let name = &s[i + 1..end];
            let v = env.get(name).cloned().unwrap_or_default();
            out.push_str(&v);
            i = end;
        } else {
            out.push('$');
            i += 1;
        }
    }
    out
}

pub fn build_env(profile_env: &indexmap::IndexMap<String, String>) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = std::env::vars().collect();
    for (k, v) in profile_env {
        m.insert(k.clone(), v.clone());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn bare_variable() {
        let e = env(&[("FOO", "bar")]);
        assert_eq!(interpolate("hello $FOO!", &e), "hello bar!");
    }

    #[test]
    fn braced_variable() {
        let e = env(&[("X", "abc")]);
        assert_eq!(interpolate("pre_${X}_post", &e), "pre_abc_post");
    }

    #[test]
    fn default_when_missing() {
        let e = env(&[]);
        assert_eq!(interpolate("${NOPE:-fallback}", &e), "fallback");
    }

    #[test]
    fn missing_bare_is_empty() {
        let e = env(&[]);
        assert_eq!(interpolate("a${NOPE}b", &e), "ab");
        assert_eq!(interpolate("a$NOPE b", &e), "a b");
    }

    #[test]
    fn literal_dollar() {
        let e = env(&[]);
        assert_eq!(interpolate("price: $5", &e), "price: $5");
    }

    #[test]
    fn unterminated_brace_is_literal() {
        let e = env(&[]);
        assert_eq!(interpolate("${broken", &e), "${broken");
    }
}
```

- [ ] **Step 2: Re-export**

Modify `crates/juballer-deck/src/config/mod.rs`:
```rust
//! Config: schema, paths, loading, hot reload.

pub mod interpolate;
pub mod schema;

pub use interpolate::{build_env, interpolate};
pub use schema::*;
```

- [ ] **Step 3: Run tests**

```
cargo test -p juballer-deck config::interpolate::tests
```

Expect 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/config/
git commit -m "feat(deck): shell-style variable interpolation for config values"
```

### Task A2.3: Config paths resolution

**Files:**
- Create: `crates/juballer-deck/src/config/paths.rs`
- Modify: `crates/juballer-deck/src/config/mod.rs`

- [ ] **Step 1: Write `paths.rs`**

```rust
//! Resolve on-disk paths for the deck config tree.

use std::path::PathBuf;

/// Default config directory: ${XDG_CONFIG_HOME:-~/.config}/juballer/deck
/// Windows: %APPDATA%/juballer/deck
pub fn default_config_dir() -> PathBuf {
    resolve_config_dir(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        std::env::var_os("APPDATA"),
        cfg!(target_os = "windows"),
    )
}

fn resolve_config_dir(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    appdata: Option<std::ffi::OsString>,
    is_windows: bool,
) -> PathBuf {
    if is_windows {
        if let Some(a) = appdata {
            return PathBuf::from(a).join("juballer").join("deck");
        }
        return PathBuf::from(".").join("juballer").join("deck");
    }
    if let Some(x) = xdg {
        return PathBuf::from(x).join("juballer").join("deck");
    }
    if let Some(h) = home {
        return PathBuf::from(h).join(".config").join("juballer").join("deck");
    }
    PathBuf::from(".").join(".config").join("juballer").join("deck")
}

pub struct DeckPaths {
    pub root: PathBuf,
    pub deck_toml: PathBuf,
    pub profiles_dir: PathBuf,
    pub plugins_dir: PathBuf,
    pub state_toml: PathBuf,
}

impl DeckPaths {
    pub fn from_root(root: PathBuf) -> Self {
        let deck_toml = root.join("deck.toml");
        let profiles_dir = root.join("profiles");
        let plugins_dir = root.join("plugins");
        let state_toml = root.join("state.toml");
        Self { root, deck_toml, profiles_dir, plugins_dir, state_toml }
    }

    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_dir.join(name)
    }

    pub fn profile_meta_toml(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("profile.toml")
    }

    pub fn profile_page_toml(&self, name: &str, page: &str) -> PathBuf {
        self.profile_dir(name).join("pages").join(format!("{page}.toml"))
    }

    pub fn profile_assets(&self, name: &str) -> PathBuf {
        self.profile_dir(name).join("assets")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_xdg() {
        let p = resolve_config_dir(Some("/x".into()), Some("/h".into()), None, false);
        assert_eq!(p, PathBuf::from("/x/juballer/deck"));
    }

    #[test]
    fn linux_home_fallback() {
        let p = resolve_config_dir(None, Some("/h".into()), None, false);
        assert_eq!(p, PathBuf::from("/h/.config/juballer/deck"));
    }

    #[test]
    fn windows_appdata() {
        let p = resolve_config_dir(None, None, Some("C:\\Users\\x\\AppData\\Roaming".into()), true);
        assert_eq!(p, PathBuf::from("C:\\Users\\x\\AppData\\Roaming\\juballer\\deck"));
    }

    #[test]
    fn deck_paths_shape() {
        let p = DeckPaths::from_root(PathBuf::from("/etc/deck"));
        assert_eq!(p.deck_toml, PathBuf::from("/etc/deck/deck.toml"));
        assert_eq!(p.profiles_dir, PathBuf::from("/etc/deck/profiles"));
        assert_eq!(p.plugins_dir, PathBuf::from("/etc/deck/plugins"));
        assert_eq!(p.state_toml, PathBuf::from("/etc/deck/state.toml"));
        assert_eq!(p.profile_meta_toml("home"), PathBuf::from("/etc/deck/profiles/home/profile.toml"));
        assert_eq!(p.profile_page_toml("home", "main"), PathBuf::from("/etc/deck/profiles/home/pages/main.toml"));
    }
}
```

- [ ] **Step 2: Re-export**

Modify `crates/juballer-deck/src/config/mod.rs`:
```rust
pub mod interpolate;
pub mod paths;
pub mod schema;

pub use interpolate::{build_env, interpolate};
pub use paths::{default_config_dir, DeckPaths};
pub use schema::*;
```

- [ ] **Step 3: Run tests**

```
cargo test -p juballer-deck config::paths::tests
```

Expect 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/config/
git commit -m "feat(deck): config path resolution (XDG + Windows AppData)"
```

### Task A2.4: Config loader — read the tree into a `ConfigTree`

**Files:**
- Create: `crates/juballer-deck/src/config/load.rs`
- Modify: `crates/juballer-deck/src/config/mod.rs`

- [ ] **Step 1: Write `load.rs`**

```rust
//! Load the deck config tree from disk into a single in-memory snapshot.

use super::paths::DeckPaths;
use super::schema::{DeckConfig, PageConfig, ProfileMeta, StateFile};
use crate::{Error, Result};
use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct ConfigTree {
    pub deck: DeckConfig,
    pub profiles: IndexMap<String, ProfileTree>,
    pub state: StateFile,
}

#[derive(Debug, Clone)]
pub struct ProfileTree {
    pub meta: ProfileMeta,
    pub pages: IndexMap<String, PageConfig>,
}

impl ConfigTree {
    pub fn load(paths: &DeckPaths) -> Result<Self> {
        let deck = load_deck(&paths.deck_toml)?;
        let mut profiles = IndexMap::new();
        if paths.profiles_dir.exists() {
            for entry in std::fs::read_dir(&paths.profiles_dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry
                    .file_name()
                    .to_str()
                    .ok_or_else(|| Error::Config(format!("non-utf8 profile dir: {:?}", entry.file_name())))?
                    .to_string();
                profiles.insert(name.clone(), load_profile(paths, &name)?);
            }
        }
        let state = if paths.state_toml.exists() {
            let s = std::fs::read_to_string(&paths.state_toml)?;
            toml::from_str(&s).map_err(|source| Error::ConfigParse {
                path: paths.state_toml.clone(),
                source,
            })?
        } else {
            StateFile::default()
        };
        Ok(Self { deck, profiles, state })
    }

    pub fn active_profile(&self) -> Result<&ProfileTree> {
        self.profiles.get(&self.deck.active_profile).ok_or_else(|| {
            Error::Config(format!("active_profile '{}' not found", self.deck.active_profile))
        })
    }
}

fn load_deck(path: &std::path::Path) -> Result<DeckConfig> {
    let s = std::fs::read_to_string(path)?;
    toml::from_str(&s).map_err(|source| Error::ConfigParse {
        path: path.to_path_buf(),
        source,
    })
}

fn load_profile(paths: &DeckPaths, name: &str) -> Result<ProfileTree> {
    let meta_path = paths.profile_meta_toml(name);
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: ProfileMeta = toml::from_str(&meta_str).map_err(|source| Error::ConfigParse {
        path: meta_path.clone(),
        source,
    })?;

    let mut pages = IndexMap::new();
    for page_name in &meta.pages {
        let p = paths.profile_page_toml(name, page_name);
        let s = std::fs::read_to_string(&p)?;
        let page: PageConfig = toml::from_str(&s).map_err(|source| Error::ConfigParse {
            path: p.clone(),
            source,
        })?;
        pages.insert(page_name.clone(), page);
    }
    Ok(ProfileTree { meta, pages })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &std::path::Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, DeckPaths) {
        let dir = tempdir().unwrap();
        let paths = DeckPaths::from_root(dir.path().to_path_buf());
        write(&paths.deck_toml, r#"
version = 1
active_profile = "homelab"

[editor]
bind = "127.0.0.1:7373"

[render]

[log]
level = "info"
"#);
        write(&paths.profile_meta_toml("homelab"), r#"
name = "homelab"
default_page = "home"
pages = ["home"]

[env]
grafana_base = "http://grafana"
"#);
        write(&paths.profile_page_toml("homelab", "home"), r#"
[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "media.playpause"
"#);
        (dir, paths)
    }

    #[test]
    fn loads_full_tree() {
        let (_dir, paths) = fixture();
        let tree = ConfigTree::load(&paths).unwrap();
        assert_eq!(tree.deck.active_profile, "homelab");
        let p = tree.active_profile().unwrap();
        assert_eq!(p.meta.default_page, "home");
        assert!(p.pages.contains_key("home"));
        assert_eq!(p.pages["home"].buttons.len(), 1);
    }

    #[test]
    fn missing_page_errors() {
        let (dir, paths) = fixture();
        std::fs::remove_file(paths.profile_page_toml("homelab", "home")).unwrap();
        let err = ConfigTree::load(&paths).unwrap_err();
        match err {
            Error::ConfigIo(_) => {}
            other => panic!("wrong error variant: {other:?}"),
        }
        drop(dir);
    }

    #[test]
    fn missing_active_profile_errors() {
        let (_dir, paths) = fixture();
        std::fs::remove_dir_all(paths.profile_dir("homelab")).unwrap();
        let tree = ConfigTree::load(&paths).unwrap();
        let err = tree.active_profile().unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong error variant: {other:?}"),
        }
    }

    #[test]
    fn state_optional() {
        let (_dir, paths) = fixture();
        let tree = ConfigTree::load(&paths).unwrap();
        assert!(tree.state.last_active_page.is_none());
    }
}
```

- [ ] **Step 2: Re-export**

Modify `crates/juballer-deck/src/config/mod.rs`:
```rust
pub mod interpolate;
pub mod load;
pub mod paths;
pub mod schema;

pub use interpolate::{build_env, interpolate};
pub use load::{ConfigTree, ProfileTree};
pub use paths::{default_config_dir, DeckPaths};
pub use schema::*;
```

- [ ] **Step 3: Run tests**

```
cargo test -p juballer-deck config::load::tests
```

Expect 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/config/
git commit -m "feat(deck): config loader reads the full deck/profiles/state tree"
```

---

## Phase 3 — State store

### Task A3.1: State store with save/load

**Files:**
- Create: `crates/juballer-deck/src/state.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `state.rs`**

```rust
//! Persisted deck state (counters, toggles, last page).
//!
//! Wraps `config::schema::StateFile` with an in-memory shadow + dirty flag, and writes
//! back to state.toml on shutdown / periodic flush.

use crate::config::schema::StateFile;
use crate::{Error, Result};
use indexmap::IndexMap;
use std::path::PathBuf;

pub struct StateStore {
    path: PathBuf,
    inner: StateFile,
    dirty: bool,
}

impl StateStore {
    pub fn open(path: PathBuf) -> Result<Self> {
        let inner = if path.exists() {
            let s = std::fs::read_to_string(&path)?;
            toml::from_str(&s).map_err(|source| Error::ConfigParse {
                path: path.clone(),
                source,
            })?
        } else {
            StateFile::default()
        };
        Ok(Self { path, inner, dirty: false })
    }

    pub fn last_active_page(&self) -> Option<&str> {
        self.inner.last_active_page.as_deref()
    }

    pub fn set_last_active_page(&mut self, page: impl Into<String>) {
        self.inner.last_active_page = Some(page.into());
        self.dirty = true;
    }

    pub fn binding(&self, id: &str) -> Option<&serde_json::Value> {
        self.inner.bindings.get(id)
    }

    pub fn set_binding(&mut self, id: impl Into<String>, value: serde_json::Value) {
        self.inner.bindings.insert(id.into(), value);
        self.dirty = true;
    }

    pub fn bindings(&self) -> &IndexMap<String, serde_json::Value> {
        &self.inner.bindings
    }

    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let s = toml::to_string(&self.inner)
            .map_err(|e| Error::Config(format!("serialize state: {e}")))?;
        std::fs::write(&self.path, s)?;
        self.dirty = false;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_empty_then_write_roundtrip() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("state.toml");
        let mut s = StateStore::open(p.clone()).unwrap();
        s.set_last_active_page("home");
        s.set_binding("home:0,0", serde_json::json!({ "count": 3 }));
        s.flush().unwrap();

        let s2 = StateStore::open(p).unwrap();
        assert_eq!(s2.last_active_page(), Some("home"));
        assert_eq!(s2.binding("home:0,0"), Some(&serde_json::json!({ "count": 3 })));
    }

    #[test]
    fn flush_noop_when_clean() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("state.toml");
        let mut s = StateStore::open(p.clone()).unwrap();
        // No sets, no flush writes.
        s.flush().unwrap();
        assert!(!p.exists());
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
mod error;
pub mod config;
pub mod state;
pub use error::{Error, Result};
pub use state::StateStore;
```

- [ ] **Step 3: Test**

```
cargo test -p juballer-deck state::tests
```

Expect 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): StateStore with dirty-tracking flush"
```

---

## Phase 4 — Event bus

### Task A4.1: Topic-addressed event bus (tokio broadcast)

**Files:**
- Create: `crates/juballer-deck/src/bus.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `bus.rs`**

```rust
//! Intra-process event bus. Actions + widgets + plugins publish/subscribe by topic.
//!
//! Wraps a tokio broadcast channel. Messages are typed as (topic, JSON value) pairs.
//! Bounded capacity (default 1024) with lagging senders overwriting old messages —
//! late subscribers may miss pre-subscription events.

use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct Event {
    pub topic: String,
    pub data: serde_json::Value,
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, topic: impl Into<String>, data: serde_json::Value) {
        // Ignore send errors: means no subscribers (tree not fully wired yet).
        let _ = self.tx.send(Event { topic: topic.into(), data });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self { Self::new(1024) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_and_receive() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        bus.publish("test.topic", serde_json::json!({ "n": 1 }));
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.topic, "test.topic");
        assert_eq!(ev.data["n"], 1);
    }

    #[tokio::test]
    async fn no_subscribers_no_error() {
        let bus = EventBus::default();
        bus.publish("nobody.listening", serde_json::json!({}));
        // No panic, no error — OK.
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
mod error;
pub mod config;
pub mod state;
pub mod bus;
pub use bus::{Event, EventBus};
pub use error::{Error, Result};
pub use state::StateStore;
```

- [ ] **Step 3: Test**

```
cargo test -p juballer-deck bus::tests
```

Expect 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): EventBus (tokio broadcast-backed topic pub/sub)"
```

---

## Phase 5 — TileHandle + IconRef

### Task A5.1: TileHandle, IconRef

**Files:**
- Create: `crates/juballer-deck/src/tile.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `tile.rs`**

```rust
//! Per-cell tile render state. Actions mutate a `TileHandle`; render layer reads the
//! underlying `TileState` at frame time.

use juballer_core::Color;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum IconRef {
    /// Relative path to an asset (profile assets/ resolved by render layer) or absolute.
    Path(PathBuf),
    /// Single emoji / short text rendered as icon.
    Emoji(String),
    /// Named icon baked into the binary (to be added case-by-case).
    Builtin(&'static str),
}

#[derive(Debug, Clone)]
pub struct TileState {
    pub icon: Option<IconRef>,
    pub label: Option<String>,
    pub bg: Option<Color>,
    pub state_color: Option<Color>,
    /// Remaining frames to flash; render layer decrements to 0.
    pub flash_until: Option<std::time::Instant>,
}

impl Default for TileState {
    fn default() -> Self {
        Self { icon: None, label: None, bg: None, state_color: None, flash_until: None }
    }
}

/// Handle given to actions during a callback. Owns a &mut to the tile state slot,
/// so mutations land immediately.
pub struct TileHandle<'a> {
    state: &'a mut TileState,
}

impl<'a> TileHandle<'a> {
    pub fn new(state: &'a mut TileState) -> Self { Self { state } }

    pub fn set_icon(&mut self, icon: IconRef) { self.state.icon = Some(icon); }
    pub fn set_label(&mut self, text: impl Into<String>) { self.state.label = Some(text.into()); }
    pub fn set_bg(&mut self, color: Color) { self.state.bg = Some(color); }
    pub fn set_state_color(&mut self, color: Color) { self.state.state_color = Some(color); }
    pub fn flash(&mut self, ms: u16) {
        self.state.flash_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(ms as u64));
    }
    pub fn state(&self) -> &TileState { self.state }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_mutates_state() {
        let mut s = TileState::default();
        {
            let mut h = TileHandle::new(&mut s);
            h.set_label("hi");
            h.set_icon(IconRef::Emoji("▶".into()));
        }
        assert_eq!(s.label.as_deref(), Some("hi"));
        match s.icon.unwrap() {
            IconRef::Emoji(e) => assert_eq!(e, "▶"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn flash_sets_future_instant() {
        let mut s = TileState::default();
        let now = std::time::Instant::now();
        {
            let mut h = TileHandle::new(&mut s);
            h.flash(200);
        }
        assert!(s.flash_until.unwrap() > now);
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
mod error;
pub mod bus;
pub mod config;
pub mod state;
pub mod tile;
pub use bus::{Event, EventBus};
pub use error::{Error, Result};
pub use state::StateStore;
pub use tile::{IconRef, TileHandle, TileState};
```

- [ ] **Step 3: Test**

```
cargo test -p juballer-deck tile::tests
```

Expect 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): TileHandle + IconRef + TileState"
```

---

## Phase 6 — Action trait + registry

### Task A6.1: Action trait + ActionCx + BuildFromArgs

**Files:**
- Create: `crates/juballer-deck/src/action/mod.rs`
- Create: `crates/juballer-deck/src/action/trait_.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `action/trait_.rs`**

```rust
//! The Action trait + ActionCx — the contract every action (built-in or plugin) follows.

use crate::bus::EventBus;
use crate::state::StateStore;
use crate::tile::TileHandle;
use crate::Result;
use indexmap::IndexMap;

/// Per-frame context passed to action callbacks.
pub struct ActionCx<'a> {
    pub cell: (u8, u8),
    pub binding_id: &'a str,
    pub tile: TileHandle<'a>,
    pub env: &'a IndexMap<String, String>,
    pub bus: &'a EventBus,
    pub state: &'a mut StateStore,
    pub rt: &'a tokio::runtime::Handle,
}

/// Long-lived object bound to one (page, row, col). Full lifecycle per spec.
pub trait Action: Send + 'static {
    fn on_will_appear(&mut self, cx: &mut ActionCx<'_>) { let _ = cx; }
    fn on_down(&mut self, cx: &mut ActionCx<'_>) { let _ = cx; }
    fn on_up(&mut self, cx: &mut ActionCx<'_>) { let _ = cx; }
    fn on_will_disappear(&mut self, cx: &mut ActionCx<'_>) { let _ = cx; }
}

/// Build-from-args trait — used by the registry to instantiate actions from TOML.
pub trait BuildFromArgs: Sized {
    fn from_args(args: &toml::Table) -> Result<Self>;
}
```

- [ ] **Step 2: Write `action/mod.rs`**

```rust
//! Action subsystem: trait, registry, built-ins.

mod registry;
mod trait_;

pub use registry::ActionRegistry;
pub use trait_::{Action, ActionCx, BuildFromArgs};

pub mod builtin;
```

Also create `crates/juballer-deck/src/action/builtin/mod.rs` as an empty stub (populated in Task A6.4):
```rust
//! Built-in actions. Plan A ships only `shell.run`; Plan B fills in the rest.

pub mod shell_run;
```

- [ ] **Step 3: Wire into lib.rs**

```rust
mod error;
pub mod action;
pub mod bus;
pub mod config;
pub mod state;
pub mod tile;
pub use action::{Action, ActionCx, ActionRegistry, BuildFromArgs};
pub use bus::{Event, EventBus};
pub use error::{Error, Result};
pub use state::StateStore;
pub use tile::{IconRef, TileHandle, TileState};
```

Defer actually compiling the trait module until A6.2 and A6.3 land — for this task, write stubs to keep the build green:

Create stubs first:

`crates/juballer-deck/src/action/registry.rs` — placeholder:
```rust
pub struct ActionRegistry;
impl ActionRegistry {
    pub fn new() -> Self { Self }
}
impl Default for ActionRegistry {
    fn default() -> Self { Self::new() }
}
```

`crates/juballer-deck/src/action/builtin/shell_run.rs` — placeholder:
```rust
//! shell.run — populated in Task A6.4.
```

- [ ] **Step 4: Verify**

```
cargo build -p juballer-deck
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): Action trait + ActionCx + BuildFromArgs skeleton"
```

### Task A6.2: ActionRegistry (real implementation)

**Files:**
- Modify: `crates/juballer-deck/src/action/registry.rs`

- [ ] **Step 1: Replace registry.rs**

```rust
//! Registry: maps action names to factory closures that instantiate actions from TOML args.

use super::trait_::{Action, BuildFromArgs};
use crate::{Error, Result};
use std::collections::HashMap;

pub type ActionFactory = Box<dyn Fn(&toml::Table) -> Result<Box<dyn Action>> + Send + Sync>;

pub struct ActionRegistry {
    factories: HashMap<&'static str, ActionFactory>,
}

impl ActionRegistry {
    pub fn new() -> Self { Self { factories: HashMap::new() } }

    pub fn register<A>(&mut self, name: &'static str)
    where
        A: Action + BuildFromArgs,
    {
        self.factories.insert(
            name,
            Box::new(|args: &toml::Table| {
                let a = A::from_args(args)?;
                Ok(Box::new(a) as Box<dyn Action>)
            }),
        );
    }

    pub fn build(&self, name: &str, args: &toml::Table) -> Result<Box<dyn Action>> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| Error::UnknownAction(name.to_string()))?;
        factory(args)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.factories.keys().copied()
    }
}

impl Default for ActionRegistry {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, ActionCx};

    struct Echo { msg: String }
    impl Action for Echo {
        fn on_down(&mut self, _cx: &mut ActionCx<'_>) {
            // no-op for test
            let _ = self.msg.len();
        }
    }
    impl BuildFromArgs for Echo {
        fn from_args(args: &toml::Table) -> Result<Self> {
            let msg = args.get("msg").and_then(|v| v.as_str()).unwrap_or("hi").to_string();
            Ok(Self { msg })
        }
    }

    #[test]
    fn register_and_build() {
        let mut r = ActionRegistry::new();
        r.register::<Echo>("test.echo");
        let mut args = toml::Table::new();
        args.insert("msg".into(), toml::Value::String("howdy".into()));
        let _a = r.build("test.echo", &args).unwrap();
        assert!(r.contains("test.echo"));
    }

    #[test]
    fn unknown_action_errors() {
        let r = ActionRegistry::new();
        let err = r.build("nope", &toml::Table::new()).unwrap_err();
        match err {
            Error::UnknownAction(name) => assert_eq!(name, "nope"),
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run tests**

```
cargo test -p juballer-deck action::registry::tests
```

Expect 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/juballer-deck/src/action/
git commit -m "feat(deck): ActionRegistry with typed factory registration"
```

### Task A6.3: First built-in action — `shell.run`

**Files:**
- Modify: `crates/juballer-deck/src/action/builtin/shell_run.rs`

- [ ] **Step 1: Write the real implementation**

```rust
//! shell.run — spawn a shell command on button-down.
//!
//! Args:
//!   cmd   : string (required) — the command line to execute via `sh -c` (unix) or `cmd /C` (windows).
//!
//! Behavior:
//!   - `on_down` spawns the command via tokio on the current runtime. Stdout/stderr inherit the deck's.
//!   - `on_will_appear` sets a default label from the cmd string (truncated to 12 chars) if no label
//!     is configured — caller-provided label overrides this in the render layer.

use crate::action::{Action, ActionCx, BuildFromArgs};
use crate::{Error, Result};

pub struct ShellRun {
    cmd: String,
}

impl BuildFromArgs for ShellRun {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let cmd = args
            .get("cmd")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("shell.run requires args.cmd (string)".into()))?
            .to_string();
        Ok(Self { cmd })
    }
}

impl Action for ShellRun {
    fn on_down(&mut self, cx: &mut ActionCx<'_>) {
        let cmd = self.cmd.clone();
        let topic = format!("action.shell.run:{}", cx.binding_id);
        let bus = cx.bus.clone();
        cx.rt.spawn(async move {
            let out = if cfg!(target_os = "windows") {
                tokio::process::Command::new("cmd").args(["/C", &cmd]).output().await
            } else {
                tokio::process::Command::new("sh").args(["-c", &cmd]).output().await
            };
            match out {
                Ok(o) => bus.publish(
                    topic,
                    serde_json::json!({
                        "status": o.status.code(),
                        "stdout_len": o.stdout.len(),
                        "stderr_len": o.stderr.len(),
                    }),
                ),
                Err(e) => bus.publish(topic, serde_json::json!({ "error": e.to_string() })),
            }
        });
        cx.tile.flash(120);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_args_requires_cmd() {
        let err = ShellRun::from_args(&toml::Table::new()).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn from_args_accepts_cmd() {
        let mut args = toml::Table::new();
        args.insert("cmd".into(), toml::Value::String("echo hi".into()));
        let a = ShellRun::from_args(&args).unwrap();
        assert_eq!(a.cmd, "echo hi");
    }
}
```

- [ ] **Step 2: Run tests**

```
cargo test -p juballer-deck action::builtin::shell_run::tests
```

Expect 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/juballer-deck/src/action/builtin/
git commit -m "feat(deck): built-in shell.run action"
```

### Task A6.4: `register_builtins` helper

**Files:**
- Create: `crates/juballer-deck/src/action/builtin/register.rs`
- Modify: `crates/juballer-deck/src/action/builtin/mod.rs`

- [ ] **Step 1: Write `register.rs`**

```rust
use super::shell_run::ShellRun;
use crate::action::ActionRegistry;

pub fn register_builtins(registry: &mut ActionRegistry) {
    registry.register::<ShellRun>("shell.run");
}
```

- [ ] **Step 2: Re-export**

Modify `crates/juballer-deck/src/action/builtin/mod.rs`:
```rust
//! Built-in actions. Plan A ships only `shell.run`; Plan B fills in the rest.

pub mod register;
pub mod shell_run;

pub use register::register_builtins;
```

- [ ] **Step 3: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/action/
git commit -m "feat(deck): register_builtins() helper for action registry"
```

---

## Phase 7 — Widget trait + registry + built-ins

### Task A7.1: Widget trait + WidgetRegistry

**Files:**
- Create: `crates/juballer-deck/src/widget/mod.rs`
- Create: `crates/juballer-deck/src/widget/trait_.rs`
- Create: `crates/juballer-deck/src/widget/registry.rs`

- [ ] **Step 1: Write `widget/trait_.rs`**

```rust
use crate::bus::EventBus;
use crate::state::StateStore;
use crate::Result;
use indexmap::IndexMap;

pub struct WidgetCx<'a> {
    pub pane: juballer_core::layout::PaneId,
    pub env: &'a IndexMap<String, String>,
    pub bus: &'a EventBus,
    pub state: &'a mut StateStore,
    pub rt: &'a tokio::runtime::Handle,
}

pub trait Widget: Send + 'static {
    fn on_will_appear(&mut self, cx: &mut WidgetCx<'_>) { let _ = cx; }
    fn on_will_disappear(&mut self, cx: &mut WidgetCx<'_>) { let _ = cx; }
    /// Render called each frame the widget's pane is visible. Returns `true` to request
    /// immediate redraw (animations).
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool;
}

pub trait WidgetBuildFromArgs: Sized {
    fn from_args(args: &toml::Table) -> Result<Self>;
}
```

- [ ] **Step 2: Write `widget/registry.rs`**

```rust
use super::trait_::{Widget, WidgetBuildFromArgs};
use crate::{Error, Result};
use std::collections::HashMap;

pub type WidgetFactory = Box<dyn Fn(&toml::Table) -> Result<Box<dyn Widget>> + Send + Sync>;

pub struct WidgetRegistry {
    factories: HashMap<&'static str, WidgetFactory>,
}

impl WidgetRegistry {
    pub fn new() -> Self { Self { factories: HashMap::new() } }

    pub fn register<W>(&mut self, name: &'static str)
    where
        W: Widget + WidgetBuildFromArgs,
    {
        self.factories.insert(
            name,
            Box::new(|args: &toml::Table| Ok(Box::new(W::from_args(args)?) as Box<dyn Widget>)),
        );
    }

    pub fn build(&self, name: &str, args: &toml::Table) -> Result<Box<dyn Widget>> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| Error::UnknownWidget(name.to_string()))?;
        factory(args)
    }

    pub fn contains(&self, name: &str) -> bool { self.factories.contains_key(name) }
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.factories.keys().copied()
    }
}

impl Default for WidgetRegistry {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 3: Write `widget/mod.rs`**

```rust
//! Widget subsystem: trait, registry, built-ins.

mod registry;
mod trait_;

pub use registry::WidgetRegistry;
pub use trait_::{Widget, WidgetBuildFromArgs, WidgetCx};

pub mod builtin;
```

Create `crates/juballer-deck/src/widget/builtin/mod.rs`:
```rust
//! Built-in widgets.

pub mod clock;
pub mod text;
pub mod register;

pub use register::register_builtins;
```

Create placeholders for `clock.rs`, `text.rs`, `register.rs` (implemented next tasks):

`clock.rs`:
```rust
//! clock widget — populated in Task A7.2.
```

`text.rs`:
```rust
//! text widget — populated in Task A7.3.
```

`register.rs`:
```rust
use crate::widget::WidgetRegistry;

pub fn register_builtins(registry: &mut WidgetRegistry) {
    // Populated as built-ins land.
    let _ = registry;
}
```

- [ ] **Step 4: Wire into lib.rs**

```rust
mod error;
pub mod action;
pub mod bus;
pub mod config;
pub mod state;
pub mod tile;
pub mod widget;
pub use action::{Action, ActionCx, ActionRegistry, BuildFromArgs};
pub use bus::{Event, EventBus};
pub use error::{Error, Result};
pub use state::StateStore;
pub use tile::{IconRef, TileHandle, TileState};
pub use widget::{Widget, WidgetCx, WidgetRegistry};
```

- [ ] **Step 5: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): Widget trait + WidgetRegistry + builtin stubs"
```

### Task A7.2: Built-in widget: `clock`

**Files:**
- Modify: `crates/juballer-deck/src/widget/builtin/clock.rs`

- [ ] **Step 1: Replace with real implementation**

```rust
//! clock widget — renders formatted local time.
//!
//! Args:
//!   format : string (default "%H:%M:%S")  -- chrono strftime format
//!
//! Uses `chrono` crate — add it to deck Cargo.toml.

use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::Result;
use chrono::Local;

pub struct Clock {
    format: String,
}

impl WidgetBuildFromArgs for Clock {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("%H:%M:%S")
            .to_string();
        Ok(Self { format })
    }
}

impl Widget for Clock {
    fn render(&mut self, ui: &mut egui::Ui, _cx: &mut WidgetCx<'_>) -> bool {
        let now = Local::now().format(&self.format).to_string();
        ui.heading(now);
        true // request next-frame redraw (animated clock)
    }
}
```

- [ ] **Step 2: Add `chrono` to deck Cargo.toml**

In `crates/juballer-deck/Cargo.toml` `[dependencies]` add:
```toml
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

- [ ] **Step 3: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): built-in clock widget (chrono-backed)"
```

### Task A7.3: Built-in widget: `text`

**Files:**
- Modify: `crates/juballer-deck/src/widget/builtin/text.rs`

- [ ] **Step 1: Replace with real implementation**

```rust
//! text widget — static text, optionally large.
//!
//! Args:
//!   content : string (required) — the text to render
//!   size    : string (optional, "small"|"body"|"heading", default "body")

use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};

pub enum TextSize { Small, Body, Heading }

pub struct Text {
    content: String,
    size: TextSize,
}

impl WidgetBuildFromArgs for Text {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("text widget requires args.content (string)".into()))?
            .to_string();
        let size = match args.get("size").and_then(|v| v.as_str()).unwrap_or("body") {
            "small" => TextSize::Small,
            "heading" => TextSize::Heading,
            _ => TextSize::Body,
        };
        Ok(Self { content, size })
    }
}

impl Widget for Text {
    fn render(&mut self, ui: &mut egui::Ui, _cx: &mut WidgetCx<'_>) -> bool {
        match self.size {
            TextSize::Small => ui.small(&self.content),
            TextSize::Body => ui.label(&self.content),
            TextSize::Heading => ui.heading(&self.content),
        };
        false
    }
}
```

- [ ] **Step 2: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add crates/juballer-deck/src/widget/
git commit -m "feat(deck): built-in text widget (small/body/heading)"
```

### Task A7.4: Register built-in widgets

**Files:**
- Modify: `crates/juballer-deck/src/widget/builtin/register.rs`

- [ ] **Step 1: Replace with real implementation**

```rust
use super::clock::Clock;
use super::text::Text;
use crate::widget::WidgetRegistry;

pub fn register_builtins(registry: &mut WidgetRegistry) {
    registry.register::<Clock>("clock");
    registry.register::<Text>("text");
}
```

- [ ] **Step 2: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add crates/juballer-deck/src/widget/builtin/register.rs
git commit -m "feat(deck): register built-in widgets (clock, text)"
```

---

## Phase 8 — Layout conversion (TOML → juballer_core::layout::Node)

### Task A8.1: Convert `LayoutNodeCfg` to core Node

**Files:**
- Create: `crates/juballer-deck/src/layout_convert.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `layout_convert.rs`**

```rust
//! Convert config `LayoutNodeCfg` trees into `juballer_core::layout::Node` trees.

use crate::config::schema::{LayoutChildCfg, LayoutChildNode, LayoutNodeCfg, SizingCfg, StackInner};
use crate::{Error, Result};
use juballer_core::layout::{Axis, Node, PaneId, Sizing};
use std::collections::HashMap;

/// Panes discovered in the tree. Returns a map of static-ified names so the deck can
/// look up widget bindings by the same name used in config.
pub struct LayoutConverted {
    pub root: Node,
    pub pane_names: Vec<String>,
}

/// Convert a `LayoutNodeCfg` into a core `Node`. PaneId is `&'static str`, so we leak
/// pane name strings to get static lifetimes. Acceptable for config-driven use: we
/// rebuild the layout on hot reload and the previous leaks remain reachable via the
/// pane_names vec (returned for subsequent teardown cycles).
pub fn convert(cfg: &LayoutNodeCfg, interner: &mut HashMap<String, &'static str>) -> Result<LayoutConverted> {
    let mut pane_names = Vec::new();
    let root = walk_node(cfg, interner, &mut pane_names)?;
    Ok(LayoutConverted { root, pane_names })
}

fn walk_node(
    cfg: &LayoutNodeCfg,
    interner: &mut HashMap<String, &'static str>,
    panes: &mut Vec<String>,
) -> Result<Node> {
    match cfg {
        LayoutNodeCfg::Pane { pane } => {
            panes.push(pane.clone());
            Ok(Node::Pane(intern(pane, interner)))
        }
        LayoutNodeCfg::Stack { dir, gap, children, .. } => {
            let axis = parse_axis(dir)?;
            let mut xs = Vec::with_capacity(children.len());
            for child in children {
                let sz = parse_sizing(&child.size);
                let node = walk_child(&child.node, interner, panes)?;
                xs.push((sz, node));
            }
            Ok(Node::Stack { dir: axis, gap_px: *gap, children: xs })
        }
    }
}

fn walk_child(
    cfg: &LayoutChildNode,
    interner: &mut HashMap<String, &'static str>,
    panes: &mut Vec<String>,
) -> Result<Node> {
    match cfg {
        LayoutChildNode::Pane { pane } => {
            panes.push(pane.clone());
            Ok(Node::Pane(intern(pane, interner)))
        }
        LayoutChildNode::Stack { stack } => walk_stack(stack, interner, panes),
    }
}

fn walk_stack(
    s: &StackInner,
    interner: &mut HashMap<String, &'static str>,
    panes: &mut Vec<String>,
) -> Result<Node> {
    let axis = parse_axis(&s.dir)?;
    let mut xs = Vec::with_capacity(s.children.len());
    for child in &s.children {
        let sz = parse_sizing(&child.size);
        let node = walk_child(&child.node, interner, panes)?;
        xs.push((sz, node));
    }
    Ok(Node::Stack { dir: axis, gap_px: s.gap, children: xs })
}

fn parse_axis(s: &str) -> Result<Axis> {
    match s {
        "horizontal" => Ok(Axis::Horizontal),
        "vertical" => Ok(Axis::Vertical),
        other => Err(Error::Config(format!("unknown axis: {other}"))),
    }
}

fn parse_sizing(s: &SizingCfg) -> Sizing {
    match s {
        SizingCfg::Fixed { fixed } => Sizing::Fixed(*fixed),
        SizingCfg::Ratio { ratio } => Sizing::Ratio(*ratio),
        SizingCfg::Auto { .. } => Sizing::Auto,
    }
}

fn intern(name: &str, interner: &mut HashMap<String, &'static str>) -> PaneId {
    if let Some(&s) = interner.get(name) {
        return s;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    interner.insert(name.to_string(), leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::*;

    #[test]
    fn convert_simple_vertical() {
        let cfg = LayoutNodeCfg::Stack {
            kind: "stack".into(),
            dir: "vertical".into(),
            gap: 10,
            children: vec![
                LayoutChildCfg {
                    size: SizingCfg::Fixed { fixed: 48 },
                    node: LayoutChildNode::Pane { pane: "header".into() },
                },
                LayoutChildCfg {
                    size: SizingCfg::Ratio { ratio: 1.0 },
                    node: LayoutChildNode::Pane { pane: "body".into() },
                },
            ],
        };
        let mut interner = HashMap::new();
        let out = convert(&cfg, &mut interner).unwrap();
        assert_eq!(out.pane_names, vec!["header", "body"]);
        match out.root {
            Node::Stack { children, .. } => {
                assert_eq!(children.len(), 2);
                match &children[0].1 {
                    Node::Pane(p) => assert_eq!(*p, "header"),
                    _ => panic!("expected pane"),
                }
            }
            _ => panic!("expected stack"),
        }
    }

    #[test]
    fn bad_axis_errors() {
        let cfg = LayoutNodeCfg::Stack {
            kind: "stack".into(),
            dir: "diagonal".into(),
            gap: 0,
            children: vec![],
        };
        let mut interner = HashMap::new();
        let err = convert(&cfg, &mut interner).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
mod error;
pub mod action;
pub mod bus;
pub mod config;
pub mod layout_convert;
pub mod state;
pub mod tile;
pub mod widget;
pub use action::{Action, ActionCx, ActionRegistry, BuildFromArgs};
pub use bus::{Event, EventBus};
pub use error::{Error, Result};
pub use state::StateStore;
pub use tile::{IconRef, TileHandle, TileState};
pub use widget::{Widget, WidgetCx, WidgetRegistry};
```

- [ ] **Step 3: Test**

```
cargo test -p juballer-deck layout_convert::tests
```

Expect 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): config LayoutNodeCfg → juballer_core layout::Node conversion"
```

---

## Phase 9 — Application shell

### Task A9.1: `DeckApp` — struct, builder, run-once path

**Files:**
- Create: `crates/juballer-deck/src/app.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `app.rs`**

```rust
//! Deck application shell. Wires juballer-core::App + registries + config + state + bus.

use crate::action::builtin::register_builtins as register_action_builtins;
use crate::action::{Action, ActionRegistry};
use crate::bus::EventBus;
use crate::config::{ConfigTree, DeckPaths};
use crate::state::StateStore;
use crate::tile::TileState;
use crate::widget::builtin::register_builtins as register_widget_builtins;
use crate::widget::{Widget, WidgetRegistry};
use crate::Result;
use indexmap::IndexMap;
use juballer_core::Color;
use std::collections::HashMap;

pub struct DeckApp {
    pub paths: DeckPaths,
    pub config: ConfigTree,
    pub state: StateStore,
    pub bus: EventBus,
    pub actions: ActionRegistry,
    pub widgets: WidgetRegistry,
    pub rt: tokio::runtime::Handle,

    /// Active page instance: bound actions indexed by (row, col)
    pub bound_actions: HashMap<(u8, u8), BoundAction>,
    /// Tile state per cell
    pub tiles: [TileState; 16],
    /// Active page name
    pub active_page: String,
    /// Layout pane name leaker for current page (kept alive while page is active)
    pub active_pane_interner: HashMap<String, &'static str>,
}

pub struct BoundAction {
    pub binding_id: String,
    pub action: Box<dyn Action>,
    pub icon: Option<String>,
    pub label: Option<String>,
}

impl DeckApp {
    pub fn bootstrap(paths: DeckPaths, rt: tokio::runtime::Handle) -> Result<Self> {
        let config = ConfigTree::load(&paths)?;
        let state = StateStore::open(paths.state_toml.clone())?;

        let mut actions = ActionRegistry::new();
        register_action_builtins(&mut actions);

        let mut widgets = WidgetRegistry::new();
        register_widget_builtins(&mut widgets);

        let active_profile = config.active_profile()?;
        let active_page = state
            .last_active_page()
            .unwrap_or(&active_profile.meta.default_page)
            .to_string();

        let mut app = Self {
            paths,
            config: config.clone(),
            state,
            bus: EventBus::default(),
            actions,
            widgets,
            rt,
            bound_actions: HashMap::new(),
            tiles: std::array::from_fn(|_| TileState::default()),
            active_page,
            active_pane_interner: HashMap::new(),
        };
        app.bind_active_page()?;
        Ok(app)
    }

    /// Build action instances for every button on the active page.
    pub fn bind_active_page(&mut self) -> Result<()> {
        self.bound_actions.clear();
        self.tiles = std::array::from_fn(|_| TileState::default());

        let profile = self.config.active_profile()?;
        let page = profile.pages.get(&self.active_page).ok_or_else(|| {
            crate::Error::Config(format!(
                "active page {} not in profile {}",
                self.active_page, profile.meta.name
            ))
        })?;

        let env = self.merged_env();
        for btn in &page.buttons {
            if btn.row >= 4 || btn.col >= 4 {
                return Err(crate::Error::Config(format!(
                    "button (row={}, col={}) out of range",
                    btn.row, btn.col
                )));
            }
            let mut args = btn.args.clone();
            interp_table(&mut args, &env);
            let action = self.actions.build(&btn.action, &args)?;
            let binding_id = format!("{}:{},{}", self.active_page, btn.row, btn.col);
            self.bound_actions.insert((btn.row, btn.col), BoundAction {
                binding_id,
                action,
                icon: btn.icon.clone(),
                label: btn.label.clone(),
            });
        }
        Ok(())
    }

    pub fn merged_env(&self) -> IndexMap<String, String> {
        let mut env = IndexMap::new();
        if let Ok(profile) = self.config.active_profile() {
            for (k, v) in &profile.meta.env {
                env.insert(k.clone(), v.clone());
            }
        }
        env
    }

    /// Render bg color from deck.toml (fallback black).
    pub fn bg_color(&self) -> Color {
        let s = self.config.deck.render.bg.as_deref().unwrap_or("#000000");
        parse_hex_color(s).unwrap_or(Color::BLACK)
    }
}

fn interp_table(table: &mut toml::Table, env: &IndexMap<String, String>) {
    // Convert profile env IndexMap to HashMap for interpolator.
    let mut h: std::collections::HashMap<String, String> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    // Also merge process env so $PORTAINER_TOKEN works.
    for (k, v) in std::env::vars() {
        h.entry(k).or_insert(v);
    }
    interp_value_in_place(table, &h);
}

fn interp_value_in_place(t: &mut toml::Table, env: &std::collections::HashMap<String, String>) {
    for (_, v) in t.iter_mut() {
        interp_one(v, env);
    }
}

fn interp_one(v: &mut toml::Value, env: &std::collections::HashMap<String, String>) {
    match v {
        toml::Value::String(s) => *s = crate::config::interpolate::interpolate(s, env),
        toml::Value::Table(t) => interp_value_in_place(t, env),
        toml::Value::Array(a) => {
            for x in a.iter_mut() { interp_one(x, env); }
        }
        _ => {}
    }
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    let bytes = match s.len() {
        6 => hex::decode(s).ok()?,
        8 => hex::decode(s).ok()?,
        _ => return None,
    };
    let a = if bytes.len() == 4 { bytes[3] } else { 0xff };
    Some(Color::rgba(bytes[0], bytes[1], bytes[2], a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &std::path::Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn bootstrap_binds_shell_action() {
        let dir = tempdir().unwrap();
        let paths = DeckPaths::from_root(dir.path().to_path_buf());
        write(&paths.deck_toml, r#"
version = 1
active_profile = "p"

[editor]
bind = "127.0.0.1:7373"

[render]

[log]
level = "info"
"#);
        write(&paths.profile_meta_toml("p"), r#"
name = "p"
default_page = "home"
pages = ["home"]
"#);
        write(&paths.profile_page_toml("p", "home"), r#"
[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "shell.run"
args = { cmd = "echo hi" }
"#);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let app = DeckApp::bootstrap(paths, rt.handle().clone()).unwrap();
        assert!(app.bound_actions.contains_key(&(0, 0)));
        assert_eq!(app.bound_actions[&(0, 0)].binding_id, "home:0,0");
    }
}
```

- [ ] **Step 2: Add `hex` to deck deps**

In `crates/juballer-deck/Cargo.toml`:
```toml
hex = "0.4"
```

- [ ] **Step 3: Wire into lib.rs**

```rust
mod error;
pub mod action;
pub mod app;
pub mod bus;
pub mod config;
pub mod layout_convert;
pub mod state;
pub mod tile;
pub mod widget;
pub use action::{Action, ActionCx, ActionRegistry, BuildFromArgs};
pub use app::DeckApp;
pub use bus::{Event, EventBus};
pub use error::{Error, Result};
pub use state::StateStore;
pub use tile::{IconRef, TileHandle, TileState};
pub use widget::{Widget, WidgetCx, WidgetRegistry};
```

- [ ] **Step 4: Test**

```
cargo test -p juballer-deck app::tests
```

Expect 1 test passes.

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): DeckApp shell — bootstrap, bind active page, env interp"
```

---

## Phase 10 — Render glue (juballer-core wiring)

### Task A10.1: Dispatch button presses to bound actions

**Files:**
- Create: `crates/juballer-deck/src/render.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `render.rs`**

```rust
//! Render glue: juballer-core frame + events → deck action dispatch + widget render.

use crate::app::DeckApp;
use crate::tile::TileHandle;
use juballer_core::input::Event;
use juballer_core::Color;

/// Per-frame entry point. Takes the DeckApp, a juballer-core Frame + events, handles input
/// dispatch and tile rendering.
pub fn on_frame(app: &mut DeckApp, frame: &mut juballer_core::Frame, events: &[Event]) {
    // 1. Handle input events → dispatch to bound actions.
    for ev in events {
        match ev {
            Event::KeyDown { row, col, .. } => {
                dispatch_down(app, *row, *col);
            }
            Event::KeyUp { row, col, .. } => {
                dispatch_up(app, *row, *col);
            }
            _ => {}
        }
    }

    // 2. Render tiles from current TileState.
    for r in 0..4u8 {
        for c in 0..4u8 {
            let tile_state = &app.tiles[(r as usize) * 4 + c as usize];
            let bg = tile_state.bg.unwrap_or(Color::rgb(0x18, 0x1a, 0x24));
            frame.grid_cell(r, c).fill(bg);
            // NOTE: icon + label rendering goes through egui in a future refinement;
            // Plan A ships with bg-color-only tile rendering so we can validate the
            // dispatch path without pulling egui into every cell. Plan B expands this
            // into full tile rendering via the egui overlay.
        }
    }
}

fn dispatch_down(app: &mut DeckApp, row: u8, col: u8) {
    let Some(bound) = app.bound_actions.get_mut(&(row, col)) else { return };
    let tile_state = &mut app.tiles[(row as usize) * 4 + col as usize];
    let env = app
        .config
        .active_profile()
        .ok()
        .map(|p| p.meta.env.clone())
        .unwrap_or_default();
    let binding_id = bound.binding_id.clone();
    let mut cx = crate::action::ActionCx {
        cell: (row, col),
        binding_id: &binding_id,
        tile: TileHandle::new(tile_state),
        env: &env,
        bus: &app.bus,
        state: &mut app.state,
        rt: &app.rt,
    };
    bound.action.on_down(&mut cx);
}

fn dispatch_up(app: &mut DeckApp, row: u8, col: u8) {
    let Some(bound) = app.bound_actions.get_mut(&(row, col)) else { return };
    let tile_state = &mut app.tiles[(row as usize) * 4 + col as usize];
    let env = app
        .config
        .active_profile()
        .ok()
        .map(|p| p.meta.env.clone())
        .unwrap_or_default();
    let binding_id = bound.binding_id.clone();
    let mut cx = crate::action::ActionCx {
        cell: (row, col),
        binding_id: &binding_id,
        tile: TileHandle::new(tile_state),
        env: &env,
        bus: &app.bus,
        state: &mut app.state,
        rt: &app.rt,
    };
    bound.action.on_up(&mut cx);
}

/// Call on_will_appear for all bound actions once, at app start and on page switch.
pub fn emit_page_appear(app: &mut DeckApp) {
    // Borrow split: iterate action keys, get mutable each time.
    let keys: Vec<(u8, u8)> = app.bound_actions.keys().copied().collect();
    for (r, c) in keys {
        let bound = app.bound_actions.get_mut(&(r, c)).unwrap();
        let tile_state = &mut app.tiles[(r as usize) * 4 + c as usize];
        let env = app
            .config
            .active_profile()
            .ok()
            .map(|p| p.meta.env.clone())
            .unwrap_or_default();
        let binding_id = bound.binding_id.clone();
        let mut cx = crate::action::ActionCx {
            cell: (r, c),
            binding_id: &binding_id,
            tile: TileHandle::new(tile_state),
            env: &env,
            bus: &app.bus,
            state: &mut app.state,
            rt: &app.rt,
        };
        bound.action.on_will_appear(&mut cx);
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
mod error;
pub mod action;
pub mod app;
pub mod bus;
pub mod config;
pub mod layout_convert;
pub mod render;
pub mod state;
pub mod tile;
pub mod widget;
pub use action::{Action, ActionCx, ActionRegistry, BuildFromArgs};
pub use app::DeckApp;
pub use bus::{Event, EventBus};
pub use error::{Error, Result};
pub use state::StateStore;
pub use tile::{IconRef, TileHandle, TileState};
pub use widget::{Widget, WidgetCx, WidgetRegistry};
```

- [ ] **Step 3: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/
git commit -m "feat(deck): render glue — on_frame dispatches press/release + fills tiles"
```

---

## Phase 11 — CLI + binary main

### Task A11.1: CLI with clap + tracing init + main entry

**Files:**
- Modify: `crates/juballer-deck/src/main.rs`
- Create: `crates/juballer-deck/src/cli.rs`
- Modify: `crates/juballer-deck/src/lib.rs`

- [ ] **Step 1: Write `cli.rs`**

```rust
//! Top-level CLI. Parses arguments + runs the deck.

use crate::app::DeckApp;
use crate::config::DeckPaths;
use crate::render::{emit_page_appear, on_frame};
use crate::Result;
use clap::{Parser, Subcommand};
use juballer_core::{App, Color, PresentMode};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "juballer-deck", version, about = "Stream-Deck-style app on GAMO2 FB9 via juballer-core")]
pub struct Cli {
    /// Path to config dir (default: ~/.config/juballer/deck)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Override active profile for this run.
    #[arg(long)]
    pub profile: Option<String>,

    /// Override monitor description for this run.
    #[arg(long)]
    pub monitor: Option<String>,

    /// Enable debug overlay from juballer-core.
    #[arg(long)]
    pub debug: bool,

    /// Render one frame and exit (headless smoke).
    #[arg(long)]
    pub once: bool,

    #[command(subcommand)]
    pub cmd: Option<SubCmd>,
}

#[derive(Subcommand, Debug)]
pub enum SubCmd {
    /// Validate config and exit.
    Check,
    /// List profiles in the config dir.
    ProfileList,
}

pub fn run(cli: Cli) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cfg_root = cli.config.unwrap_or_else(crate::config::default_config_dir);
    let paths = DeckPaths::from_root(cfg_root);

    match cli.cmd {
        Some(SubCmd::Check) => {
            let tree = crate::config::ConfigTree::load(&paths)?;
            println!("deck OK. profiles: {:?}", tree.profiles.keys().collect::<Vec<_>>());
            return Ok(());
        }
        Some(SubCmd::ProfileList) => {
            let tree = crate::config::ConfigTree::load(&paths)?;
            for (name, p) in &tree.profiles {
                println!("{}  {}", name, p.meta.description);
            }
            return Ok(());
        }
        None => {}
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let mut deck = DeckApp::bootstrap(paths, rt.handle().clone())?;
    if let Some(profile) = cli.profile {
        deck.config.deck.active_profile = profile;
        deck.bind_active_page()?;
    }

    let monitor_desc = cli.monitor.or_else(|| deck.config.deck.render.monitor_desc.clone());

    let present_mode = match deck.config.deck.render.present_mode.as_deref() {
        Some("immediate") => PresentMode::Immediate,
        Some("mailbox") => PresentMode::Mailbox,
        _ => PresentMode::Fifo,
    };

    let mut builder = App::builder()
        .title("juballer-deck")
        .present_mode(present_mode)
        .bg_color(deck.bg_color())
        .controller_vid_pid(0x1973, 0x0011); // GAMO2 FB9 (Plan D makes this configurable)
    if let Some(m) = &monitor_desc { builder = builder.on_monitor(m.clone()); }
    let mut app = builder.build()?;

    if cli.debug { app.set_debug(true); }

    // Top-layout integration (call App::set_top_layout from deck page config) lands in Plan B.

    emit_page_appear(&mut deck);
    let mut frame_count = 0u64;
    let once = cli.once;

    app.run(move |frame, events| {
        on_frame(&mut deck, frame, events);
        frame_count += 1;
        if once && frame_count >= 1 {
            // juballer-core has no clean exit from draw callback; signal via panic for --once.
            // This is a smoke-test only path; real runs don't use --once.
            std::process::exit(0);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_basic_flags() {
        let c = Cli::try_parse_from(["juballer-deck", "--config", "/x", "--debug", "--once"]).unwrap();
        assert_eq!(c.config.unwrap(), PathBuf::from("/x"));
        assert!(c.debug);
        assert!(c.once);
    }

    #[test]
    fn parses_check_subcmd() {
        let c = Cli::try_parse_from(["juballer-deck", "check"]).unwrap();
        assert!(matches!(c.cmd, Some(SubCmd::Check)));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
mod error;
pub mod action;
pub mod app;
pub mod bus;
pub mod cli;
pub mod config;
pub mod layout_convert;
pub mod render;
pub mod state;
pub mod tile;
pub mod widget;
pub use action::{Action, ActionCx, ActionRegistry, BuildFromArgs};
pub use app::DeckApp;
pub use bus::{Event, EventBus};
pub use cli::{Cli, SubCmd};
pub use error::{Error, Result};
pub use state::StateStore;
pub use tile::{IconRef, TileHandle, TileState};
pub use widget::{Widget, WidgetCx, WidgetRegistry};
```

- [ ] **Step 3: Replace `src/main.rs`**

```rust
use clap::Parser;
use juballer_deck::cli;

fn main() {
    let args = cli::Cli::parse();
    if let Err(e) = cli::run(args) {
        eprintln!("juballer-deck: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 4: Verify**

```
cargo build -p juballer-deck
cargo test -p juballer-deck cli::tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expect 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/juballer-deck/
git commit -m "feat(deck): CLI with clap + main entry + tracing init"
```

---

## Phase 12 — Fixture + smoke test

### Task A12.1: Fixture profile for smoke testing

**Files:**
- Create: `crates/juballer-deck/tests/fixtures/minimal/deck.toml`
- Create: `crates/juballer-deck/tests/fixtures/minimal/profiles/demo/profile.toml`
- Create: `crates/juballer-deck/tests/fixtures/minimal/profiles/demo/pages/home.toml`
- Create: `crates/juballer-deck/tests/smoke.rs`

- [ ] **Step 1: Write the fixture files**

`crates/juballer-deck/tests/fixtures/minimal/deck.toml`:
```toml
version = 1
active_profile = "demo"

[editor]
bind = "127.0.0.1:7374"

[render]
bg = "#0b0d12"

[log]
level = "info"
```

`crates/juballer-deck/tests/fixtures/minimal/profiles/demo/profile.toml`:
```toml
name = "demo"
description = "Plan A smoke"
default_page = "home"
pages = ["home"]
```

`crates/juballer-deck/tests/fixtures/minimal/profiles/demo/pages/home.toml`:
```toml
[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "shell.run"
args = { cmd = "echo smoke-ok" }
icon = "▶"
label = "echo"
```

- [ ] **Step 2: Write `tests/smoke.rs`**

```rust
//! End-to-end smoke: load the fixture config, bootstrap the deck, dispatch a synthetic
//! button-0,0 press, wait for the tokio side-effect, assert bus received the result event.

use juballer_deck::action::ActionCx;
use juballer_deck::config::DeckPaths;
use juballer_deck::tile::{TileHandle, TileState};
use juballer_deck::DeckApp;
use std::path::PathBuf;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_action_fires_and_publishes_result() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal");
    let paths = DeckPaths::from_root(fixture);
    let rt = tokio::runtime::Handle::current();

    let mut deck = DeckApp::bootstrap(paths, rt.clone()).unwrap();
    assert!(deck.bound_actions.contains_key(&(0, 0)));

    let mut rx = deck.bus.subscribe();

    // Simulate a button-down by calling on_down directly.
    let bound = deck.bound_actions.get_mut(&(0, 0)).unwrap();
    let tile_state = &mut TileState::default();
    let env = deck
        .config
        .active_profile()
        .unwrap()
        .meta
        .env
        .clone();
    let binding_id = bound.binding_id.clone();
    {
        let mut cx = ActionCx {
            cell: (0, 0),
            binding_id: &binding_id,
            tile: TileHandle::new(tile_state),
            env: &env,
            bus: &deck.bus,
            state: &mut deck.state,
            rt: &rt,
        };
        bound.action.on_down(&mut cx);
    }

    // Wait for the spawned task to complete and publish to the bus.
    let ev = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("bus recv timed out")
        .expect("bus channel closed");
    assert_eq!(ev.topic, "action.shell.run:home:0,0");
    assert!(ev.data.get("status").is_some() || ev.data.get("error").is_some());
}
```

- [ ] **Step 3: Run**

```
cargo test -p juballer-deck --test smoke
```

Expect 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/tests/
git commit -m "test(deck): end-to-end smoke — shell.run fires + publishes to bus"
```

---

## Phase 13 — Config hot reload

### Task A13.1: notify-based watcher with debounce

**Files:**
- Create: `crates/juballer-deck/src/config/watch.rs`
- Modify: `crates/juballer-deck/src/config/mod.rs`

- [ ] **Step 1: Write `watch.rs`**

```rust
//! Config directory watcher. Debounces notify events + emits a single `ReloadRequested`
//! token on the output channel per quiet interval.

use crate::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub enum ReloadSignal { ReloadRequested }

/// Spawn a watcher + debouncer thread. Returns a receiver that emits `ReloadRequested`
/// signals, debounced at `quiet_for`.
pub fn watch(root: &Path, quiet_for: Duration) -> Result<(RecommendedWatcher, mpsc::Receiver<ReloadSignal>)> {
    let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = raw_tx.send(res);
    })
    .map_err(|e| crate::Error::Config(format!("watcher init: {e}")))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| crate::Error::Config(format!("watch {root:?}: {e}")))?;

    let (out_tx, out_rx) = mpsc::channel::<ReloadSignal>();
    std::thread::Builder::new()
        .name("juballer-deck-config-watch".into())
        .spawn(move || {
            let mut last_event: Option<Instant> = None;
            loop {
                match raw_rx.recv_timeout(quiet_for / 2) {
                    Ok(Ok(_ev)) => {
                        last_event = Some(Instant::now());
                    }
                    Ok(Err(_)) => {} // notify error, keep going
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(t) = last_event {
                            if t.elapsed() >= quiet_for {
                                let _ = out_tx.send(ReloadSignal::ReloadRequested);
                                last_event = None;
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .map_err(|e| crate::Error::Config(format!("watch thread: {e}")))?;

    Ok((watcher, out_rx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_trigger_reload_signal() {
        let dir = tempdir().unwrap();
        // Seed a file
        std::fs::write(dir.path().join("deck.toml"), "version = 1\nactive_profile = \"x\"\n").unwrap();
        let (_w, rx) = watch(dir.path(), Duration::from_millis(200)).unwrap();

        // Modify file, wait for signal.
        std::thread::sleep(Duration::from_millis(50));
        std::fs::write(dir.path().join("deck.toml"), "version = 1\nactive_profile = \"y\"\n").unwrap();

        let got = rx.recv_timeout(Duration::from_secs(3)).expect("no reload signal");
        assert!(matches!(got, ReloadSignal::ReloadRequested));
    }
}
```

- [ ] **Step 2: Re-export**

Modify `crates/juballer-deck/src/config/mod.rs`:
```rust
pub mod interpolate;
pub mod load;
pub mod paths;
pub mod schema;
pub mod watch;

pub use interpolate::{build_env, interpolate};
pub use load::{ConfigTree, ProfileTree};
pub use paths::{default_config_dir, DeckPaths};
pub use schema::*;
pub use watch::{watch, ReloadSignal};
```

- [ ] **Step 3: Test**

```
cargo test -p juballer-deck config::watch::tests
```

Expect 1 test passes (may take up to 3 seconds).

- [ ] **Step 4: Commit**

```bash
git add crates/juballer-deck/src/config/
git commit -m "feat(deck): notify-based config watcher with debounce"
```

### Task A13.2: Wire watcher into the run loop

**Files:**
- Modify: `crates/juballer-deck/src/cli.rs`

- [ ] **Step 1: Add reload plumbing to `run`**

In `run()`, after building `DeckApp` and before `app.run(...)`:

```rust
// Spawn config watcher; signals land on a stdlib channel.
let (_watcher, reload_rx) = crate::config::watch::watch(&deck.paths.root.clone(), std::time::Duration::from_millis(300))?;
let reload_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
{
    let flag = reload_flag.clone();
    std::thread::Builder::new()
        .name("juballer-deck-reload-signal".into())
        .spawn(move || {
            for _sig in reload_rx.iter() {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .expect("reload signal thread");
}
```

In the draw closure, before `on_frame`, check + apply reload:

```rust
if reload_flag.swap(false, std::sync::atomic::Ordering::Relaxed) {
    if let Err(e) = crate::config::ConfigTree::load(&deck.paths) {
        tracing::warn!("reload: config load failed: {e}");
    } else if let Ok(new_config) = crate::config::ConfigTree::load(&deck.paths) {
        deck.config = new_config;
        if let Err(e) = deck.bind_active_page() {
            tracing::warn!("reload: rebind failed: {e}");
        } else {
            tracing::info!("reload: config applied");
            crate::render::emit_page_appear(&mut deck);
        }
    }
}
```

Full updated `run` closure (replace the `app.run(...)` call with):

```rust
app.run(move |frame, events| {
    if reload_flag.swap(false, std::sync::atomic::Ordering::Relaxed) {
        match crate::config::ConfigTree::load(&deck.paths) {
            Ok(new_config) => {
                deck.config = new_config;
                if let Err(e) = deck.bind_active_page() {
                    tracing::warn!("reload: rebind failed: {e}");
                } else {
                    tracing::info!("reload: config applied");
                    crate::render::emit_page_appear(&mut deck);
                }
            }
            Err(e) => tracing::warn!("reload: config load failed: {e}"),
        }
    }
    on_frame(&mut deck, frame, events);
    frame_count += 1;
    if once && frame_count >= 1 {
        std::process::exit(0);
    }
})?;
```

- [ ] **Step 2: Verify**

```
cargo build -p juballer-deck
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: Commit**

```bash
git add crates/juballer-deck/src/cli.rs
git commit -m "feat(deck): wire config watcher into run loop; live reload on file change"
```

---

## Self-Review Checklist (run after writing plan)

- [x] Every spec section relevant to "foundation + one smoke action" has a task.
- [x] No placeholders of the "TBD / fill in later" variety.
- [x] Types are consistent across tasks: `Action`, `ActionCx`, `ActionRegistry`, `BuildFromArgs`, `Widget`, `WidgetCx`, `WidgetRegistry`, `WidgetBuildFromArgs`, `DeckApp`, `TileHandle`, `TileState`, `IconRef`, `ConfigTree`, `ProfileTree`, `DeckPaths`, `StateStore`, `EventBus`, `Event`.
- [x] File paths are explicit.
- [x] Every task ends with a commit with a Conventional Commits subject.
- [x] Phase ordering: workspace → error → config schema → interp → paths → loader → state → bus → tile → action trait/registry/builtin → widget trait/registry/builtins → layout convert → app shell → render glue → CLI → fixture + smoke → hot reload.
- [x] Smoke test uses the fixture and validates: config loads, bound_actions populated, shell.run fires, bus receives result.

## Out-of-scope (handled by later plans)

- Widget rendering via juballer-egui overlay in tiles — current Plan A renders only cell bg color. Tile icon + label rendering lands in Plan B (which also introduces per-cell egui via the overlay).
- Top-region widget layout → tree-aware rendering loop. Plan A's `on_frame` only fills grid cells; Plan B adds `set_top_layout` + EguiOverlay integration to drive widgets.
- Remaining built-in actions (46 more). Plan B.
- Remaining built-in widgets (10 more). Plan C.
- Plugin host / protocol / Python SDK. Plan D.
- Web-based config editor (axum + TS SPA). Plan E.
- Perf contract tests, latency probes, hardware smoke. Plan F.
