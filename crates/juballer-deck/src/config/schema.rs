//! TOML-backed config schema for the deck, profiles, pages, and persisted state.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    #[serde(default)]
    pub rhythm: RhythmConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorConfig {
    pub bind: String,
    #[serde(default)]
    pub require_auth: bool,
    #[serde(default = "editor_enabled_default")]
    pub enabled: bool,
}

fn editor_enabled_default() -> bool {
    true
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:7373".into(),
            require_auth: false,
            enabled: true,
        }
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
    /// Theme preset: "mocha" (default) | "latte".
    #[serde(default)]
    pub theme: Option<String>,
}

/// Rhythm-mode config. Holds the default charts directory used when `play`
/// is invoked with no `CHART` argument, plus player-tunable runtime
/// settings (audio offset, volume, SFX toggle) editable via the in-app
/// `settings` subcommand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RhythmConfig {
    /// Default directory to hand to the picker when `play` is run with no
    /// `CHART` argument. Passed through verbatim; no canonicalization.
    #[serde(default)]
    pub charts_dir: Option<PathBuf>,
    /// Persistent audio offset in ms. Positive = audio lags input → subtract
    /// from music_time at play time. Mirrors the `--audio-offset-ms` CLI flag.
    #[serde(default)]
    pub audio_offset_ms: i32,
    /// Master song volume, 0.0..=1.0. Clamped to this range on load + write.
    #[serde(default = "rhythm_volume_default")]
    pub volume: f32,
    /// Whether per-grade hit SFX play. `false` is equivalent to `--mute-sfx`.
    #[serde(default = "rhythm_sfx_enabled_default")]
    pub sfx_enabled: bool,
    /// Opt-in gameplay modifiers. Persist across runs.
    #[serde(default)]
    pub mods: ModConfig,
    /// How many ms before a note's hit time the approach visual begins.
    /// Higher values give the player more reaction time at the cost of
    /// more on-screen clutter on dense charts. 1000 (one full second)
    /// is the default matching classic 4×4-grid rhythm games.
    #[serde(default = "rhythm_lead_in_ms_default")]
    pub lead_in_ms: u32,
    /// Optional directory holding user asset overrides (marker packs,
    /// sound effects, etc). When set, the rhythm loader probes here
    /// BEFORE falling back to the bundled defaults under
    /// `CARGO_MANIFEST_DIR/../../assets/`. Intended path:
    /// `~/.config/juballer/rhythm/assets/`.
    #[serde(default)]
    pub asset_dir: Option<PathBuf>,
    /// Optional list of paths rendered as the HUD top-bar background
    /// behind everything else. Each entry can be either:
    ///   * a `.wgsl` shader — compiled via the tile-shader pipeline with
    ///     a dedicated `BackgroundUniforms` struct exposing music /
    ///     input channels (music_ms, bpm, combo, life, last_grade,
    ///     held_mask, …)
    ///   * an image (`.png` / `.jpg` / `.jpeg`) — stretched to fill the
    ///     top region with aspect-ignoring scaling
    ///
    /// If multiple entries are configured, the runtime picks one per
    /// chart deterministically (`hash(chart_path) % len`) so each song
    /// gets a stable backdrop but the player rotates through the set
    /// as they play different songs. Empty = HUD backdrop stays plain.
    #[serde(default)]
    pub backgrounds: Vec<PathBuf>,
    /// Optional fixed index into [`Self::backgrounds`]. When `Some(i)`
    /// every chart uses `backgrounds[i]` — think of it as "favourite" /
    /// "pinned". When `None`, the deterministic-hash mix above applies.
    /// Out-of-range indices fall back to mix mode + a `warn!` log.
    #[serde(default)]
    pub background_index: Option<usize>,
}

fn rhythm_lead_in_ms_default() -> u32 {
    1000
}

/// Per-run gameplay modifiers. Cheap to clone; loaded once at session
/// start and read by the rhythm loop. New flags append here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModConfig {
    /// When true, the life bar is clamped above 0 so the player can
    /// never reach the FAILED banner mid-song.
    #[serde(default)]
    pub no_fail: bool,
}

fn rhythm_volume_default() -> f32 {
    1.0
}

fn rhythm_sfx_enabled_default() -> bool {
    true
}

impl Default for RhythmConfig {
    fn default() -> Self {
        Self {
            charts_dir: None,
            audio_offset_ms: 0,
            volume: rhythm_volume_default(),
            sfx_enabled: rhythm_sfx_enabled_default(),
            mods: ModConfig::default(),
            lead_in_ms: rhythm_lead_in_ms_default(),
            asset_dir: None,
            backgrounds: Vec::new(),
            background_index: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogConfig {
    pub level: String,
    /// Per-file size cap (MB). Once a daily log file crosses this it
    /// gets truncated at startup. Default 50.
    #[serde(default = "default_log_max_file_mb")]
    pub max_file_mb: u64,
    /// How many daily log files to retain. Older files are deleted at
    /// startup. Default 7.
    #[serde(default = "default_log_max_files")]
    pub max_files: usize,
    /// Override log directory. Default `<config>/logs/`.
    #[serde(default)]
    pub dir: Option<std::path::PathBuf>,
}

fn default_log_max_file_mb() -> u64 {
    50
}
fn default_log_max_files() -> usize {
    7
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            max_file_mb: default_log_max_file_mb(),
            max_files: default_log_max_files(),
            dir: None,
        }
    }
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
    /// Logical row count of the page. If > 4 the page supports vertical scrolling.
    /// Default 4 keeps pre-existing single-screen pages identical.
    #[serde(default = "default_logical_dim")]
    pub logical_rows: u8,
    /// Logical column count of the page. If > 4 the page supports horizontal scrolling.
    #[serde(default = "default_logical_dim")]
    pub logical_cols: u8,
    /// Logical row indices that are pinned — they render at the matching physical row and
    /// never scroll. E.g. `pinned_rows = [3]` keeps logical row 3 in physical row 3.
    #[serde(default)]
    pub pinned_rows: Vec<u8>,
    /// Logical column indices pinned to the matching physical column.
    #[serde(default)]
    pub pinned_cols: Vec<u8>,
}

fn default_logical_dim() -> u8 {
    4
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
    Pane {
        pane: String,
    },
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
    #[serde(default)]
    pub shader: Option<TileShaderCfg>,
    /// Optional stable logical name for this button. Plugins can target tiles
    /// by name via `Message::TileSetByName` — independent of (row, col).
    #[serde(default)]
    pub name: Option<String>,
}

/// Tile shader/video source declared in a button config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TileShaderCfg {
    Wgsl { wgsl: String },
    Video { video: String },
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
        let s = r##"
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
"##;
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
        assert_eq!(
            p.buttons[1].args.get("cmd").unwrap().as_str().unwrap(),
            "notify-send hi"
        );
    }

    #[test]
    fn button_name_parses() {
        let s = r#"
[meta]
title = "x"

[[button]]
row = 1
col = 1
action = "deck.page_goto"
args = { page = "discord:overview" }
name = "discord_unread"
icon = "💬"
label = "discord"
"#;
        let p: PageConfig = toml::from_str(s).unwrap();
        assert_eq!(p.buttons[0].name.as_deref(), Some("discord_unread"));
    }

    #[test]
    fn page_meta_defaults_to_4x4_no_pins() {
        let s = r#"
[meta]
title = "plain"
"#;
        let p: PageConfig = toml::from_str(s).unwrap();
        assert_eq!(p.meta.logical_rows, 4);
        assert_eq!(p.meta.logical_cols, 4);
        assert!(p.meta.pinned_rows.is_empty());
        assert!(p.meta.pinned_cols.is_empty());
    }

    #[test]
    fn page_meta_scroll_and_pin_fields_parse() {
        let s = r#"
[meta]
title = "dev"
logical_rows = 8
logical_cols = 4
pinned_rows = [3]

[[button]]
row = 5
col = 2
action = "shell.run"
args = { cmd = "echo hi" }
"#;
        let p: PageConfig = toml::from_str(s).unwrap();
        assert_eq!(p.meta.logical_rows, 8);
        assert_eq!(p.meta.logical_cols, 4);
        assert_eq!(p.meta.pinned_rows, vec![3u8]);
        assert!(p.meta.pinned_cols.is_empty());
        assert_eq!(p.buttons[0].row, 5);
    }

    #[test]
    fn deck_toml_with_rhythm_charts_dir_roundtrips() {
        let s = r##"
version = 1
active_profile = "homelab"

[rhythm]
charts_dir = "/home/jsabella/charts"
"##;
        let c: DeckConfig = toml::from_str(s).unwrap();
        assert_eq!(
            c.rhythm.charts_dir.as_deref(),
            Some(std::path::Path::new("/home/jsabella/charts"))
        );
        let back = toml::to_string(&c).unwrap();
        let c2: DeckConfig = toml::from_str(&back).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn deck_toml_without_rhythm_section_parses_cleanly() {
        let s = r##"
version = 1
active_profile = "homelab"
"##;
        let c: DeckConfig = toml::from_str(s).unwrap();
        assert!(c.rhythm.charts_dir.is_none());
    }

    #[test]
    fn rhythm_config_defaults_when_section_absent() {
        // With no [rhythm] block at all, defaults must kick in for the
        // player-tunable fields — the settings UI reads these on first
        // launch before anything is ever written back.
        let s = r##"
version = 1
active_profile = "homelab"
"##;
        let c: DeckConfig = toml::from_str(s).unwrap();
        assert_eq!(c.rhythm.audio_offset_ms, 0);
        assert!((c.rhythm.volume - 1.0).abs() < 1e-9);
        assert!(c.rhythm.sfx_enabled);
    }

    #[test]
    fn rhythm_config_partial_section_defaults_missing_fields() {
        // A [rhythm] section with only some fields set must still produce
        // defaults for the others. This guards against breaking existing
        // deck.toml files that predate the new fields.
        let s = r##"
version = 1
active_profile = "homelab"

[rhythm]
charts_dir = "/some/dir"
"##;
        let c: DeckConfig = toml::from_str(s).unwrap();
        assert_eq!(c.rhythm.audio_offset_ms, 0);
        assert!((c.rhythm.volume - 1.0).abs() < 1e-9);
        assert!(c.rhythm.sfx_enabled);
    }

    #[test]
    fn rhythm_config_roundtrips_all_fields() {
        let s = r##"
version = 1
active_profile = "homelab"

[rhythm]
charts_dir = "/charts"
audio_offset_ms = -25
volume = 0.5
sfx_enabled = false
"##;
        let c: DeckConfig = toml::from_str(s).unwrap();
        assert_eq!(c.rhythm.audio_offset_ms, -25);
        assert!((c.rhythm.volume - 0.5).abs() < 1e-6);
        assert!(!c.rhythm.sfx_enabled);
        let back = toml::to_string(&c).unwrap();
        let c2: DeckConfig = toml::from_str(&back).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn state_file_roundtrip() {
        let mut s = StateFile {
            last_active_page: Some("home".into()),
            ..Default::default()
        };
        s.bindings
            .insert("home:0,0".into(), serde_json::json!({ "count": 5 }));
        let toml_str = toml::to_string(&s).unwrap();
        let back: StateFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(s, back);
    }
}
