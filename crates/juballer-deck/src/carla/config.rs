//! TOML schema + loader for Carla configurations and preset files.
//!
//! Schema covers three feature phases. Phase 1 (input cells) is the
//! only one whose semantics ship today; Phase 2 (display cells) and
//! Phase 3 (preset library + preset picker) are parsed and validated
//! so users can write forward-compatible files now and the modes will
//! "wake up" once the runtime catches up.
//!
//! ## Cell anatomy
//!
//! Cells are intentionally *optional* on every binding. A cell can
//! attach to **any** combination of:
//!
//! - `tap`     — fires on a short-press release
//! - `hold`    — fires after the long-press threshold is crossed
//! - `display` — read-only mirror of one or more plugin parameters
//!   (Phase 2; parsed but not yet rendered)
//!
//! All three are independent. A cell with only `display` is a passive
//! readout; a cell with `tap` + `hold` lets a single key drive two
//! different parameter changes; a cell with all three (e.g. tap = bump
//! gain, hold = bypass, display = current peak) consolidates an entire
//! channel strip on one key.
//!
//! ## File locations
//!
//! - Configurations: `~/.config/juballer/carla/configs/<name>.toml`
//! - Presets: `~/.config/juballer/carla/presets/<category>/<name>.preset.toml`

use crate::config::atomic::atomic_write;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Default Carla OSC port — matches Carla's built-in server.
pub const DEFAULT_CARLA_PORT: u16 = 22752;
/// Default Carla OSC host.
pub const DEFAULT_CARLA_HOST: &str = "127.0.0.1";

/// Top-level Carla configuration file. One file per saved configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Configuration {
    /// User-facing name. Free-form; falls back to the file stem when missing.
    #[serde(default)]
    pub name: Option<String>,
    /// One-line description shown in the picker overlay.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub carla: CarlaTarget,
    /// One or more pages of cell bindings. The runtime paginates between
    /// them via the bottom-row PREV / NEXT cells.
    #[serde(default, rename = "page")]
    pub pages: Vec<Page>,
}

/// Where to send OSC messages.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CarlaTarget {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for CarlaTarget {
    fn default() -> Self {
        Self {
            host: DEFAULT_CARLA_HOST.to_string(),
            port: DEFAULT_CARLA_PORT,
        }
    }
}

fn default_host() -> String {
    DEFAULT_CARLA_HOST.to_string()
}

fn default_port() -> u16 {
    DEFAULT_CARLA_PORT
}

/// One sub-page of cell bindings. Renders 12 cells (rows 0-2); the
/// bottom row is reserved for navigation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Page {
    /// Optional page title; appears in the top-region HUD.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, rename = "cell")]
    pub cells: Vec<Cell>,
}

/// A single cell binding. All trigger slots are optional — a cell with
/// no slots is a placeholder that paints blank. Validation enforces
/// per-slot required fields, not whole-cell required slots, so empty
/// cells are legal.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Cell {
    pub row: u8,
    pub col: u8,
    /// Override label drawn on the cell. Defaults to a short auto-label
    /// derived from the active triggers when omitted.
    #[serde(default)]
    pub label: Option<String>,
    /// Fires on short-press release.
    #[serde(default)]
    pub tap: Option<Action>,
    /// Fires when the press duration crosses the long-press threshold.
    #[serde(default)]
    pub hold: Option<Action>,
    /// Read-only mirror of plugin parameter(s). Phase 2.
    #[serde(default)]
    pub display: Option<DisplayBinding>,
}

/// A single parameter-write that runs in response to a press / hold.
/// The flat shape (every per-mode field as `Option<…>`) lets users
/// scan TOML files top-down without nested tables; [`Action::validate`]
/// trades compile-time field checking for an explicit pass at load.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Action {
    /// Carla plugin slot — string name or numeric index. Names are
    /// resolved to indices by the OSC client at startup.
    pub plugin: PluginRef,
    /// Parameter the action drives. Required for every `mode` except
    /// the preset modes (which target the plugin as a whole).
    #[serde(default)]
    pub param: Option<ParamRef>,
    pub mode: ActionMode,

    // Numeric input modes ----------------------------------------------
    /// Increment / decrement size for `bump-up` / `bump-down`.
    #[serde(default)]
    pub step: Option<f32>,
    /// Lower clamp.
    #[serde(default)]
    pub min: Option<f32>,
    /// Upper clamp.
    #[serde(default)]
    pub max: Option<f32>,
    /// Absolute value to write, used by `set` and as the on-state for
    /// `momentary` if `on_value` is unset.
    #[serde(default)]
    pub value: Option<f32>,
    /// Toggle / momentary "on" value (default 1.0).
    #[serde(default)]
    pub on_value: Option<f32>,
    /// Toggle / momentary "off" value (default 0.0).
    #[serde(default)]
    pub off_value: Option<f32>,

    // Carousel modes ---------------------------------------------------
    /// Discrete value list for `carousel-next` / `carousel-prev`.
    /// Pressing the cell advances (or rewinds) one position and writes
    /// the new value. The list wraps at both ends.
    #[serde(default)]
    pub values: Option<Vec<f32>>,
    /// Optional UI labels, parallel to `values`. Length must match
    /// `values` when present; the HUD renders the active label.
    #[serde(default)]
    pub value_labels: Option<Vec<String>>,

    // Preset modes (Phase 3) -------------------------------------------
    /// `load-preset`: name of a preset to apply on press.
    #[serde(default)]
    pub preset: Option<String>,
    /// `open-preset-picker`: optional category filter.
    #[serde(default)]
    pub category: Option<String>,
}

/// What an action does to its parameter when triggered.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum ActionMode {
    // Phase 1
    /// Add `step` to the current parameter value, clamped to `[min, max]`.
    BumpUp,
    /// Subtract `step` from the current parameter value, clamped to `[min, max]`.
    BumpDown,
    /// Flip the parameter between `on_value` and `off_value` based on
    /// the cached current value.
    Toggle,
    /// Press-down sends `on_value`, press-up sends `off_value`. The
    /// runtime honours this by emitting both events; mapping it to the
    /// `hold` slot only emits on the threshold crossing (the off-event
    /// is sent on release as usual).
    Momentary,
    /// Write the constant `value` regardless of current state.
    Set,
    /// Cycle forward through the discrete `values` list (wraps at end).
    /// Used for enum-like params: filter type, oversampling factor, etc.
    CarouselNext,
    /// Cycle backward through the same list (wraps at start).
    CarouselPrev,
    // Phase 3
    LoadPreset,
    OpenPresetPicker,
}

impl ActionMode {
    /// True for modes the Phase 1 runtime knows how to dispatch.
    pub fn is_phase1(self) -> bool {
        matches!(
            self,
            Self::BumpUp
                | Self::BumpDown
                | Self::Toggle
                | Self::Momentary
                | Self::Set
                | Self::CarouselNext
                | Self::CarouselPrev
        )
    }

    pub fn is_preset(self) -> bool {
        matches!(self, Self::LoadPreset | Self::OpenPresetPicker)
    }
}

/// Read-only display binding (Phase 2). Parses but does not render in
/// Phase 1; preserved so configs written today survive the upgrade.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DisplayBinding {
    pub mode: DisplayMode,
    /// Default source param for `value` / `text` / `meter` modes.
    #[serde(default)]
    pub source_param: Option<ParamRef>,
    /// Tuner mode: param exposing detected frequency (Hz).
    #[serde(default)]
    pub freq_param: Option<ParamRef>,
    /// Tuner mode: param exposing the detected note (string or MIDI #).
    #[serde(default)]
    pub note_param: Option<ParamRef>,
    /// Tuner mode: param exposing pitch deviation in cents.
    #[serde(default)]
    pub cents_param: Option<ParamRef>,
    /// Meter mode: optional peak / hold companion param.
    #[serde(default)]
    pub peak_param: Option<ParamRef>,
    /// Plugin scoping the source params. When omitted, `source_param`
    /// etc. are interpreted against the cell-level plugin (carried by
    /// the active page); for now display bindings need their own.
    #[serde(default)]
    pub plugin: Option<PluginRef>,
    /// Optional `format!`-style template for `value` / `text` rendering.
    #[serde(default)]
    pub format: Option<String>,
    /// Display poll rate. Defaults to 30 Hz at the renderer when None.
    #[serde(default)]
    pub poll_hz: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayMode {
    Tuner,
    Meter,
    Value,
    Text,
    ActivePresetName,
}

/// A reference to either a Carla plugin slot or a parameter on one,
/// keyed by either a stable string name or the raw numeric index Carla
/// assigns. Names are easier to write but require a name → index
/// resolve against a live Carla; indices are brittle when plugin
/// updates change ordering. The OSC client resolves names lazily and
/// caches the result.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum PluginRef {
    Index(u32),
    Name(String),
}

/// Same shape as [`PluginRef`]; a separate alias documents intent.
pub type ParamRef = PluginRef;

/// A preset file (Phase 3). Lives at
/// `~/.config/juballer/carla/presets/<category>/<name>.preset.toml`.
/// Category = parent directory name.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Preset {
    /// User-facing name. Free-form; falls back to file stem.
    #[serde(default)]
    pub name: Option<String>,
    /// One-line description.
    #[serde(default)]
    pub description: Option<String>,
    /// Plugin name pattern this preset targets — typically the plugin's
    /// public name (e.g. `"CabXr"`). Loaded into whichever Carla slot
    /// matches at apply-time.
    pub target_plugin: String,
    /// Parameter snapshot: `(name_or_index, value)` pairs. Applied via
    /// `/Carla/<plugin>/set_parameter_value`.
    #[serde(default, rename = "param")]
    pub params: Vec<PresetParam>,
    /// Filesystem paths the preset references (impulse responses,
    /// sample banks, etc). Applied via `/Carla/<plugin>/set_custom_data`.
    #[serde(default, rename = "file")]
    pub files: Vec<PresetFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PresetParam {
    pub name: ParamRef,
    pub value: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PresetFile {
    /// LV2 custom-data key. Plugin-specific.
    pub key: String,
    /// Absolute or `~`-relative path to the file.
    pub path: PathBuf,
}

/// Validation errors surfaced by the various `validate()` methods.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("cell ({row},{col}): row must be 0..=3 and col must be 0..=3")]
    OutOfRange { row: u8, col: u8 },
    #[error("cell ({row},{col}) sits on the reserved bottom row used by the navigation bar")]
    NavRowConflict { row: u8, col: u8 },
    #[error("cell ({row},{col}) [{slot}, mode={mode:?}]: missing required field `{field}`")]
    MissingField {
        row: u8,
        col: u8,
        slot: TriggerSlot,
        mode: ActionMode,
        field: &'static str,
    },
    #[error("cell ({row},{col}) [{slot}, mode={mode:?}]: `min` must be <= `max`")]
    MinMaxInverted {
        row: u8,
        col: u8,
        slot: TriggerSlot,
        mode: ActionMode,
    },
    #[error(
        "cell ({row},{col}) [{slot}, carousel]: `value_labels` length must equal `values` length"
    )]
    CarouselLabelCountMismatch { row: u8, col: u8, slot: TriggerSlot },
    #[error("cell ({row},{col}) [display, mode={mode:?}]: missing required field `{field}`")]
    DisplayMissingField {
        row: u8,
        col: u8,
        mode: DisplayMode,
        field: &'static str,
    },
    #[error("two cells share coordinates ({row},{col}) on the same page")]
    DuplicateCell { row: u8, col: u8 },
}

/// Identifies which trigger slot caused a validation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSlot {
    Tap,
    Hold,
}

impl std::fmt::Display for TriggerSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tap => write!(f, "tap"),
            Self::Hold => write!(f, "hold"),
        }
    }
}

impl Cell {
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errs = Vec::new();
        if self.row > 3 || self.col > 3 {
            errs.push(ValidationError::OutOfRange {
                row: self.row,
                col: self.col,
            });
            return errs;
        }
        if self.row == 3 {
            errs.push(ValidationError::NavRowConflict {
                row: self.row,
                col: self.col,
            });
        }
        if let Some(action) = &self.tap {
            errs.extend(action.validate(self.row, self.col, TriggerSlot::Tap));
        }
        if let Some(action) = &self.hold {
            errs.extend(action.validate(self.row, self.col, TriggerSlot::Hold));
        }
        if let Some(disp) = &self.display {
            errs.extend(disp.validate(self.row, self.col));
        }
        errs
    }

    /// True for "this cell does literally nothing." Used by the
    /// renderer to suppress click feedback on placeholder tiles.
    pub fn is_blank(&self) -> bool {
        self.tap.is_none() && self.hold.is_none() && self.display.is_none()
    }
}

impl Action {
    pub fn validate(&self, row: u8, col: u8, slot: TriggerSlot) -> Vec<ValidationError> {
        let mut errs = Vec::new();
        let need = |field: &'static str, errs: &mut Vec<ValidationError>| {
            errs.push(ValidationError::MissingField {
                row,
                col,
                slot,
                mode: self.mode,
                field,
            });
        };
        match self.mode {
            ActionMode::BumpUp | ActionMode::BumpDown => {
                if self.param.is_none() {
                    need("param", &mut errs);
                }
                if self.step.is_none() {
                    need("step", &mut errs);
                }
                if let (Some(lo), Some(hi)) = (self.min, self.max) {
                    if lo > hi {
                        errs.push(ValidationError::MinMaxInverted {
                            row,
                            col,
                            slot,
                            mode: self.mode,
                        });
                    }
                }
            }
            ActionMode::Toggle | ActionMode::Momentary => {
                if self.param.is_none() {
                    need("param", &mut errs);
                }
            }
            ActionMode::Set => {
                if self.param.is_none() {
                    need("param", &mut errs);
                }
                if self.value.is_none() {
                    need("value", &mut errs);
                }
            }
            ActionMode::CarouselNext | ActionMode::CarouselPrev => {
                if self.param.is_none() {
                    need("param", &mut errs);
                }
                match &self.values {
                    None => need("values", &mut errs),
                    Some(v) if v.is_empty() => need("values", &mut errs),
                    Some(_) => {}
                }
                if let (Some(values), Some(labels)) = (&self.values, &self.value_labels) {
                    if values.len() != labels.len() {
                        errs.push(ValidationError::CarouselLabelCountMismatch { row, col, slot });
                    }
                }
            }
            ActionMode::LoadPreset => {
                if self.preset.is_none() {
                    need("preset", &mut errs);
                }
            }
            ActionMode::OpenPresetPicker => { /* category optional */ }
        }
        errs
    }

    /// On-state for toggle / momentary (default 1.0).
    pub fn resolved_on_value(&self) -> f32 {
        self.on_value.or(self.value).unwrap_or(1.0)
    }

    /// Off-state for toggle / momentary (default 0.0).
    pub fn resolved_off_value(&self) -> f32 {
        self.off_value.unwrap_or(0.0)
    }
}

impl DisplayBinding {
    pub fn validate(&self, row: u8, col: u8) -> Vec<ValidationError> {
        let mut errs = Vec::new();
        let need = |field: &'static str, errs: &mut Vec<ValidationError>| {
            errs.push(ValidationError::DisplayMissingField {
                row,
                col,
                mode: self.mode,
                field,
            });
        };
        match self.mode {
            DisplayMode::Tuner => {
                if self.freq_param.is_none() {
                    need("freq_param", &mut errs);
                }
            }
            DisplayMode::Meter => {
                if self.source_param.is_none() {
                    need("source_param", &mut errs);
                }
            }
            DisplayMode::Value | DisplayMode::Text => {
                if self.source_param.is_none() {
                    need("source_param", &mut errs);
                }
            }
            DisplayMode::ActivePresetName => { /* nothing required */ }
        }
        errs
    }
}

impl Configuration {
    /// Load a configuration TOML from disk. Validates the parsed result
    /// — invalid configs become `Err` rather than runtime surprises.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&bytes)
            .map_err(|e| crate::Error::Config(format!("carla config {}: {e}", path.display())))?;
        cfg.validate()
            .map_err(|errs| crate::Error::Config(format_validation(path, &errs)))?;
        Ok(cfg)
    }

    /// Atomic write through `crate::config::atomic`.
    pub fn save(&self, path: &Path) -> Result<()> {
        let body = toml::to_string_pretty(self).map_err(|e| {
            crate::Error::Config(format!("carla config encode {}: {e}", path.display()))
        })?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        atomic_write(path, body.as_bytes())?;
        Ok(())
    }

    pub fn validate(&self) -> std::result::Result<(), Vec<ValidationError>> {
        let mut errs = Vec::new();
        for page in &self.pages {
            errs.extend(page.validate());
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }

    /// Display name. Falls back to a placeholder string used in the
    /// picker when no `name = "…"` is set.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("(unnamed)")
    }
}

impl Page {
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for cell in &self.cells {
            errs.extend(cell.validate());
            if !seen.insert((cell.row, cell.col)) {
                errs.push(ValidationError::DuplicateCell {
                    row: cell.row,
                    col: cell.col,
                });
            }
        }
        errs
    }
}

impl Preset {
    /// Load a preset TOML from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read_to_string(path)?;
        toml::from_str(&bytes)
            .map_err(|e| crate::Error::Config(format!("carla preset {}: {e}", path.display())))
    }

    /// Atomic save.
    pub fn save(&self, path: &Path) -> Result<()> {
        let body = toml::to_string_pretty(self).map_err(|e| {
            crate::Error::Config(format!("carla preset encode {}: {e}", path.display()))
        })?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        atomic_write(path, body.as_bytes())?;
        Ok(())
    }

    /// Pull a category name from the preset's parent directory. Mirrors
    /// the rhythm picker's `pack_of` heuristic.
    pub fn category_from_path(path: &Path) -> Option<String> {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            .map(str::to_owned)
    }
}

/// Default config-library directory. Resolved relative to the standard
/// juballer config root so multiple deck profiles see the same library.
pub fn default_configs_dir() -> PathBuf {
    crate::config::paths::default_config_dir()
        .join("carla")
        .join("configs")
}

/// Default preset-library directory.
pub fn default_presets_dir() -> PathBuf {
    crate::config::paths::default_config_dir()
        .join("carla")
        .join("presets")
}

fn format_validation(path: &Path, errs: &[ValidationError]) -> String {
    let mut buf = format!("carla config {}: validation failed:", path.display());
    for e in errs {
        buf.push_str("\n  - ");
        buf.push_str(&e.to_string());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(mode: ActionMode) -> Action {
        Action {
            plugin: PluginRef::Name("Plug".into()),
            param: Some(PluginRef::Name("Wet".into())),
            mode,
            step: None,
            min: None,
            max: None,
            value: None,
            on_value: None,
            off_value: None,
            values: None,
            value_labels: None,
            preset: None,
            category: None,
        }
    }

    fn cell(row: u8, col: u8) -> Cell {
        Cell {
            row,
            col,
            label: None,
            tap: None,
            hold: None,
            display: None,
        }
    }

    #[test]
    fn carla_target_defaults_match_carla_built_in_server() {
        let t = CarlaTarget::default();
        assert_eq!(t.host, "127.0.0.1");
        assert_eq!(t.port, 22752);
    }

    #[test]
    fn cell_with_no_triggers_is_blank_but_validates() {
        let c = cell(0, 0);
        assert!(c.is_blank());
        assert!(c.validate().is_empty());
    }

    #[test]
    fn cell_validate_rejects_out_of_range_coordinates() {
        let c = cell(4, 0);
        let errs = c.validate();
        assert!(matches!(
            errs[0],
            ValidationError::OutOfRange { row: 4, .. }
        ));
    }

    #[test]
    fn cell_validate_rejects_nav_row() {
        let c = cell(3, 0);
        let errs = c.validate();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::NavRowConflict { row: 3, .. })));
    }

    #[test]
    fn tap_validate_requires_step_for_bump_up() {
        let mut c = cell(0, 0);
        c.tap = Some(action(ActionMode::BumpUp));
        let errs = c.validate();
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::MissingField {
                slot: TriggerSlot::Tap,
                field: "step",
                ..
            }
        )));
    }

    #[test]
    fn hold_validate_requires_value_for_set() {
        let mut c = cell(0, 0);
        let mut a = action(ActionMode::Set);
        a.value = None;
        c.hold = Some(a);
        let errs = c.validate();
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::MissingField {
                slot: TriggerSlot::Hold,
                field: "value",
                ..
            }
        )));
    }

    #[test]
    fn cell_can_have_both_tap_and_hold_actions() {
        let mut c = cell(0, 0);
        let mut tap = action(ActionMode::BumpUp);
        tap.step = Some(0.05);
        c.tap = Some(tap);
        let mut hold = action(ActionMode::Set);
        hold.value = Some(0.0);
        c.hold = Some(hold);
        assert!(c.validate().is_empty());
        assert!(!c.is_blank());
    }

    #[test]
    fn display_validate_requires_freq_param_for_tuner() {
        let mut c = cell(0, 0);
        c.display = Some(DisplayBinding {
            mode: DisplayMode::Tuner,
            source_param: None,
            freq_param: None,
            note_param: None,
            cents_param: None,
            peak_param: None,
            plugin: None,
            format: None,
            poll_hz: None,
        });
        let errs = c.validate();
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::DisplayMissingField {
                field: "freq_param",
                ..
            }
        )));
    }

    #[test]
    fn page_validate_detects_duplicate_coordinates() {
        let page = Page {
            title: None,
            cells: vec![cell(0, 0), cell(0, 0)],
        };
        let errs = page.validate();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateCell { row: 0, col: 0 })));
    }

    #[test]
    fn action_mode_phase1_excludes_preset_modes() {
        assert!(ActionMode::BumpUp.is_phase1());
        assert!(ActionMode::Momentary.is_phase1());
        assert!(ActionMode::CarouselNext.is_phase1());
        assert!(ActionMode::CarouselPrev.is_phase1());
        assert!(!ActionMode::LoadPreset.is_phase1());
        assert!(ActionMode::OpenPresetPicker.is_preset());
    }

    #[test]
    fn carousel_validate_requires_non_empty_values_list() {
        let mut c = cell(0, 0);
        c.tap = Some(action(ActionMode::CarouselNext));
        let errs = c.validate();
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::MissingField {
                field: "values",
                ..
            }
        )));

        let mut c2 = cell(0, 1);
        let mut a = action(ActionMode::CarouselNext);
        a.values = Some(vec![]);
        c2.tap = Some(a);
        let errs = c2.validate();
        assert!(errs.iter().any(|e| matches!(
            e,
            ValidationError::MissingField {
                field: "values",
                ..
            }
        )));
    }

    #[test]
    fn carousel_validate_rejects_label_count_mismatch() {
        let mut c = cell(0, 0);
        let mut a = action(ActionMode::CarouselNext);
        a.values = Some(vec![0.0, 1.0, 2.0]);
        a.value_labels = Some(vec!["A".into(), "B".into()]);
        c.tap = Some(a);
        let errs = c.validate();
        assert!(errs
            .iter()
            .any(|e| matches!(e, ValidationError::CarouselLabelCountMismatch { .. })));
    }

    #[test]
    fn carousel_round_trip_preserves_values_and_labels() {
        let body = r#"
            [[page]]
            [[page.cell]]
            row = 0
            col = 0
            [page.cell.tap]
            plugin = "EQ"
            param = "FilterType"
            mode = "carousel-next"
            values = [0.0, 1.0, 2.0, 3.0]
            value_labels = ["LP", "HP", "BP", "BR"]
        "#;
        let cfg: Configuration = toml::from_str(body).unwrap();
        cfg.validate().unwrap();
        let tap = cfg.pages[0].cells[0].tap.as_ref().unwrap();
        assert_eq!(tap.values.as_deref().unwrap().len(), 4);
        assert_eq!(tap.value_labels.as_deref().unwrap()[3], "BR");
    }

    #[test]
    fn round_trip_minimal_config_through_toml() {
        let body = r#"
            name = "Minimal"

            [[page]]
            title = "Reverb"

            [[page.cell]]
            row = 0
            col = 0

            [page.cell.tap]
            plugin = "Roomy"
            param = "Wet"
            mode = "bump-up"
            step = 0.05
        "#;
        let cfg: Configuration = toml::from_str(body).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.pages.len(), 1);
        let cell = &cfg.pages[0].cells[0];
        let tap = cell.tap.as_ref().unwrap();
        assert_eq!(tap.mode, ActionMode::BumpUp);
        match &tap.plugin {
            PluginRef::Name(n) => assert_eq!(n, "Roomy"),
            _ => panic!("expected named plugin ref"),
        }
        let encoded = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Configuration = toml::from_str(&encoded).unwrap();
        parsed.validate().unwrap();
    }

    #[test]
    fn round_trip_cell_with_tap_hold_and_display() {
        let body = r#"
            [[page]]
            [[page.cell]]
            row = 0
            col = 0
            label = "Channel 1"

            [page.cell.tap]
            plugin = "Compressor"
            param = "Threshold"
            mode = "bump-up"
            step = 1.0

            [page.cell.hold]
            plugin = "Compressor"
            param = "Bypass"
            mode = "toggle"

            [page.cell.display]
            mode = "meter"
            source_param = "Gain Reduction"
        "#;
        let cfg: Configuration = toml::from_str(body).unwrap();
        cfg.validate().unwrap();
        let c = &cfg.pages[0].cells[0];
        assert!(c.tap.is_some());
        assert!(c.hold.is_some());
        assert!(c.display.is_some());
        assert!(!c.is_blank());
    }

    #[test]
    fn plugin_ref_accepts_either_string_or_index() {
        let body = r#"
            [[page]]
            [[page.cell]]
            row = 0
            col = 0
            [page.cell.tap]
            plugin = 7
            param = 3
            mode = "set"
            value = 0.5
        "#;
        let cfg: Configuration = toml::from_str(body).unwrap();
        cfg.validate().unwrap();
        assert_eq!(
            cfg.pages[0].cells[0].tap.as_ref().unwrap().plugin,
            PluginRef::Index(7)
        );
    }

    #[test]
    fn configuration_load_surfaces_validation_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(
            &path,
            r#"
                [[page]]
                [[page.cell]]
                row = 0
                col = 0
                [page.cell.tap]
                plugin = "x"
                param = "y"
                mode = "bump-up"
            "#,
        )
        .unwrap();
        let err = Configuration::load(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("step"), "expected step error, got: {msg}");
    }

    #[test]
    fn preset_round_trip_preserves_param_and_file_lists() {
        let body = r#"
            name = "Vintage Marshall 4x12"
            target_plugin = "CabXr"

            [[param]]
            name = "Mix"
            value = 0.7

            [[file]]
            key = "ir_file"
            path = "/srv/ir/marshall.wav"
        "#;
        let p: Preset = toml::from_str(body).unwrap();
        assert_eq!(p.target_plugin, "CabXr");
        assert_eq!(p.params.len(), 1);
        assert_eq!(p.files[0].key, "ir_file");
        let encoded = toml::to_string_pretty(&p).unwrap();
        let parsed: Preset = toml::from_str(&encoded).unwrap();
        assert_eq!(parsed.target_plugin, "CabXr");
    }

    #[test]
    fn category_from_path_returns_parent_directory_name() {
        let p = Path::new("/tmp/cabs/vintage/marshall.preset.toml");
        assert_eq!(Preset::category_from_path(p).as_deref(), Some("vintage"));
    }
}
