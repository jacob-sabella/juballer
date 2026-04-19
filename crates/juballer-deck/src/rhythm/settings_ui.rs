//! In-app rhythm settings editor. Fullscreen 4×4 grid where each row is
//! one tunable field (audio offset, volume, SFX enable). Columns:
//!
//! ```text
//!   col 0   col 1           col 2   col 3
//! ┌───────┬───────────────┬───────┬───────┐
//! │   –   │ audio offset  │   +   │       │   row 0
//! ├───────┼───────────────┼───────┼───────┤
//! │   –   │ volume        │   +   │       │   row 1
//! ├───────┼───────────────┼───────┼───────┤
//! │   –   │ sfx           │   +   │       │   row 2
//! ├───────┼───────────────┼───────┼───────┤
//! │       │               │       │ EXIT  │   row 3
//! └───────┴───────────────┴───────┴───────┘
//! ```
//!
//! Pressing `–` decrements / toggles off, `+` increments / toggles on.
//! Cell (3,3) exits and writes back to `deck.toml`, preserving any
//! sections/keys the editor doesn't know about. All other cells are no-ops.
//!
//! Write-back mirrors the pattern in
//! [`crate::editor::server::api_activate_profile`] — read → `toml::Value`
//! edit → atomic write — so comments on keys we don't touch are lost but
//! unrelated sections and values are not.
//!
//! The state machine lives outside the winit loop so it can be unit-tested
//! without a GPU / winit context.

use crate::config::{atomic_write, DeckPaths, RhythmConfig};
use crate::{Error, Result};
use juballer_core::input::Event;
use juballer_core::{App, Color, Frame, PresentMode};
use juballer_egui::EguiOverlay;
use std::path::Path;

/// Step applied to `audio_offset_ms` per `+`/`–` tap. 5ms matches the
/// granularity the calibrator prints recommendations at.
pub const AUDIO_OFFSET_STEP_MS: i32 = 5;

/// Step applied to `volume` per `+`/`–` tap. 0.05 gives twenty taps across
/// the full 0..=1 range — coarse enough to be fast, fine enough for
/// meaningful adjustment.
pub const VOLUME_STEP: f32 = 0.05;

/// Which row a given setting lives on in the grid. Kept as a concrete enum
/// so the state-machine tests (`apply_cell`) can enumerate intent without
/// coupling to row/col pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    AudioOffset,
    Volume,
    Sfx,
}

impl SettingRow {
    fn from_row(row: u8) -> Option<Self> {
        match row {
            0 => Some(Self::AudioOffset),
            1 => Some(Self::Volume),
            2 => Some(Self::Sfx),
            _ => None,
        }
    }
}

/// Outcome of applying a tap to the editor state. Lets callers distinguish
/// "write on exit" from "update HUD" without threading separate booleans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Changed,
    Exit,
}

/// Editable mirror of [`RhythmConfig`]'s tunable fields.
///
/// Separate from `RhythmConfig` itself so tests can assert exact
/// clamps/toggles without rebuilding the whole deck config.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsState {
    pub audio_offset_ms: i32,
    pub volume: f32,
    pub sfx_enabled: bool,
}

impl SettingsState {
    /// Seed from the on-disk config. Volume is clamped on the way in so a
    /// hand-edited `deck.toml` with a bogus value can't blow the UI up.
    pub fn from_config(cfg: &RhythmConfig) -> Self {
        Self {
            audio_offset_ms: cfg.audio_offset_ms,
            volume: cfg.volume.clamp(0.0, 1.0),
            sfx_enabled: cfg.sfx_enabled,
        }
    }

    /// Apply a tap on `(row, col)`. Returns the resulting action so the
    /// caller can decide when to repaint / write.
    ///
    /// Layout, restated from the module docstring:
    /// - (3,3): exit (writes on exit)
    /// - col 0, rows 0..=2: decrement / toggle off
    /// - col 2, rows 0..=2: increment / toggle on
    /// - every other cell: no-op
    pub fn apply_cell(&mut self, row: u8, col: u8) -> Action {
        if row == 3 && col == 3 {
            return Action::Exit;
        }
        let Some(setting) = SettingRow::from_row(row) else {
            return Action::None;
        };
        match col {
            0 => {
                self.adjust(setting, -1);
                Action::Changed
            }
            2 => {
                self.adjust(setting, 1);
                Action::Changed
            }
            _ => Action::None,
        }
    }

    fn adjust(&mut self, setting: SettingRow, sign: i32) {
        match setting {
            SettingRow::AudioOffset => {
                self.audio_offset_ms = self
                    .audio_offset_ms
                    .saturating_add(sign * AUDIO_OFFSET_STEP_MS);
            }
            SettingRow::Volume => {
                let next = self.volume + sign as f32 * VOLUME_STEP;
                self.volume = next.clamp(0.0, 1.0);
            }
            SettingRow::Sfx => {
                // `–` toggles off, `+` toggles on. Idempotent — tapping `+`
                // twice in a row leaves sfx on.
                self.sfx_enabled = sign > 0;
            }
        }
    }
}

/// Merge `state` back into the on-disk `deck.toml` at `deck_path`,
/// preserving other sections. Writes atomically via [`atomic_write`].
///
/// Strategy mirrors `api_activate_profile`: load the raw TOML, mutate the
/// `[rhythm]` table in-place via `toml::Value`, re-serialise. We don't
/// round-trip through `DeckConfig` because that would strip any
/// unknown/forward-compat keys the user might have added by hand.
pub fn write_rhythm_section(deck_path: &Path, state: &SettingsState) -> Result<()> {
    let current = std::fs::read_to_string(deck_path)
        .map_err(|e| Error::Config(format!("settings: read {}: {e}", deck_path.display())))?;
    let mut doc: toml::Value = toml::from_str(&current)
        .map_err(|e| Error::Config(format!("settings: parse deck.toml: {e}")))?;
    let table = doc
        .as_table_mut()
        .ok_or_else(|| Error::Config("settings: deck.toml is not a table".to_string()))?;
    let rhythm_entry = table
        .entry("rhythm".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let rhythm = rhythm_entry
        .as_table_mut()
        .ok_or_else(|| Error::Config("settings: [rhythm] is not a table".to_string()))?;
    rhythm.insert(
        "audio_offset_ms".into(),
        toml::Value::Integer(state.audio_offset_ms.into()),
    );
    rhythm.insert(
        "volume".into(),
        toml::Value::Float(f64::from(state.volume.clamp(0.0, 1.0))),
    );
    rhythm.insert(
        "sfx_enabled".into(),
        toml::Value::Boolean(state.sfx_enabled),
    );
    let serialized = toml::to_string_pretty(&doc)
        .map_err(|e| Error::Config(format!("settings: toml encode: {e}")))?;
    atomic_write(deck_path, serialized.as_bytes())
        .map_err(|e| Error::Config(format!("settings: atomic_write: {e}")))?;
    Ok(())
}

/// Run the settings overlay. Bootstraps the config tree at `paths`,
/// seeds a [`SettingsState`], opens a fullscreen winit window, paints the
/// grid, and exits (writing back) on (3,3).
pub fn run(paths: &DeckPaths) -> Result<()> {
    let tree = crate::config::ConfigTree::load(paths)?;
    let initial = SettingsState::from_config(&tree.deck.rhythm);
    let mut state = initial.clone();

    let mut app = App::builder()
        .title("juballer — settings")
        .present_mode(PresentMode::Fifo)
        .bg_color(Color::BLACK)
        .controller_vid_pid(0x1973, 0x0011)
        .build()?;
    app.set_debug(false);

    let mut overlay = EguiOverlay::new();
    let deck_path = paths.deck_toml.clone();

    app.run(move |frame, events| {
        paint_backgrounds(frame);
        draw_overlay(frame, &mut overlay, &state);

        for ev in events {
            match ev {
                Event::KeyDown { row, col, .. } => match state.apply_cell(*row, *col) {
                    Action::Exit => {
                        if state != initial {
                            if let Err(e) = write_rhythm_section(&deck_path, &state) {
                                tracing::warn!(
                                    target: "juballer::rhythm::settings",
                                    "write failed: {e}"
                                );
                            } else {
                                tracing::info!(
                                    target: "juballer::rhythm::settings",
                                    "settings saved → {}",
                                    deck_path.display()
                                );
                            }
                        }
                        super::exit::exit(0);
                    }
                    Action::Changed | Action::None => {}
                },
                Event::Unmapped { key, .. } if key.0 == "NAMED_Escape" => {
                    // Escape = cancel without writing.
                    super::exit::exit(0);
                }
                Event::Quit => super::exit::exit(0),
                _ => {}
            }
        }
    })?;
    Ok(())
}

fn paint_backgrounds(frame: &mut Frame) {
    let minus = Color::rgba(0x30, 0x18, 0x1c, 0xff);
    let plus = Color::rgba(0x18, 0x30, 0x1c, 0xff);
    let label = Color::rgba(0x1a, 0x1e, 0x2a, 0xff);
    let unused = Color::rgba(0x0a, 0x0a, 0x10, 0xff);
    let exit = Color::rgba(0x40, 0x10, 0x14, 0xff);
    for r in 0..4u8 {
        for c in 0..4u8 {
            let color = if r == 3 && c == 3 {
                exit
            } else if r == 3 {
                unused
            } else {
                match c {
                    0 => minus,
                    1 => label,
                    2 => plus,
                    _ => unused,
                }
            };
            frame.grid_cell(r, c).fill(color);
        }
    }
}

fn draw_overlay(frame: &mut Frame, overlay: &mut EguiOverlay, state: &SettingsState) {
    let cell_rects = *frame.cell_rects();
    overlay.draw(frame, |rc| {
        let rows: [(SettingRow, &str, String); 3] = [
            (
                SettingRow::AudioOffset,
                "AUDIO OFFSET",
                format!("{:+} ms", state.audio_offset_ms),
            ),
            (
                SettingRow::Volume,
                "VOLUME",
                format!("{:.0}%", state.volume * 100.0),
            ),
            (
                SettingRow::Sfx,
                "SFX",
                if state.sfx_enabled {
                    "ON".into()
                } else {
                    "OFF".into()
                },
            ),
        ];
        for (i, (_setting, title, value)) in rows.iter().enumerate() {
            // `–` label in col 0.
            let minus_rect = cell_rects[i * 4];
            draw_centered_text(
                rc.ctx(),
                minus_rect,
                egui::Id::new(("settings_minus", i)),
                "–",
                36.0,
                egui::Color32::from_rgb(240, 160, 170),
            );
            // Label + current value in col 1.
            let label_rect = cell_rects[i * 4 + 1];
            let area_id = egui::Id::new(("settings_label", i));
            egui::Area::new(area_id)
                .fixed_pos(egui::pos2(
                    label_rect.x as f32 + 12.0,
                    label_rect.y as f32 + 12.0,
                ))
                .order(egui::Order::Foreground)
                .show(rc.ctx(), |ui| {
                    ui.set_width(label_rect.w as f32 - 24.0);
                    let painter = ui.painter();
                    let anchor = ui.cursor().left_top();
                    painter.text(
                        anchor + egui::vec2(0.0, 0.0),
                        egui::Align2::LEFT_TOP,
                        *title,
                        egui::FontId::proportional(16.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                    painter.text(
                        anchor + egui::vec2(0.0, 28.0),
                        egui::Align2::LEFT_TOP,
                        value,
                        egui::FontId::monospace(22.0),
                        egui::Color32::WHITE,
                    );
                });
            // `+` label in col 2.
            let plus_rect = cell_rects[i * 4 + 2];
            draw_centered_text(
                rc.ctx(),
                plus_rect,
                egui::Id::new(("settings_plus", i)),
                "+",
                36.0,
                egui::Color32::from_rgb(160, 240, 180),
            );
        }
        // Exit label in (3,3).
        let exit_rect = cell_rects[15];
        draw_centered_text(
            rc.ctx(),
            exit_rect,
            egui::Id::new("settings_exit"),
            "EXIT",
            20.0,
            egui::Color32::from_rgb(240, 120, 130),
        );
    });
}

fn draw_centered_text(
    ctx: &egui::Context,
    rect: juballer_core::Rect,
    id: egui::Id,
    text: &str,
    size: f32,
    color: egui::Color32,
) {
    egui::Area::new(id)
        .fixed_pos(egui::pos2(rect.x as f32, rect.y as f32))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_width(rect.w as f32);
            ui.set_height(rect.h as f32);
            let painter = ui.painter();
            let center = ui.max_rect().center();
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(size),
                color,
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> SettingsState {
        SettingsState {
            audio_offset_ms: 0,
            volume: 0.5,
            sfx_enabled: true,
        }
    }

    #[test]
    fn apply_cell_increments_and_decrements_audio_offset() {
        // Row 0 col 2 = `+`, row 0 col 0 = `–`.
        let mut s = seed();
        assert_eq!(s.apply_cell(0, 2), Action::Changed);
        assert_eq!(s.audio_offset_ms, AUDIO_OFFSET_STEP_MS);
        assert_eq!(s.apply_cell(0, 0), Action::Changed);
        assert_eq!(s.apply_cell(0, 0), Action::Changed);
        assert_eq!(s.audio_offset_ms, -AUDIO_OFFSET_STEP_MS);
    }

    #[test]
    fn apply_cell_clamps_volume_to_unit_range() {
        // Starting at 0.5, twenty `+` taps should pin at 1.0, not overshoot.
        let mut s = seed();
        for _ in 0..20 {
            s.apply_cell(1, 2);
        }
        assert!((s.volume - 1.0).abs() < 1e-6, "volume = {}", s.volume);
        // And twenty `–` taps from there should pin at 0.0.
        for _ in 0..40 {
            s.apply_cell(1, 0);
        }
        assert!(s.volume.abs() < 1e-6, "volume = {}", s.volume);
    }

    #[test]
    fn apply_cell_toggles_sfx() {
        let mut s = seed();
        assert!(s.sfx_enabled);
        assert_eq!(s.apply_cell(2, 0), Action::Changed);
        assert!(!s.sfx_enabled);
        assert_eq!(s.apply_cell(2, 2), Action::Changed);
        assert!(s.sfx_enabled);
        // Idempotent on consecutive taps in the same direction.
        s.apply_cell(2, 2);
        assert!(s.sfx_enabled);
        s.apply_cell(2, 0);
        s.apply_cell(2, 0);
        assert!(!s.sfx_enabled);
    }

    #[test]
    fn apply_cell_exit_cell_returns_exit() {
        let mut s = seed();
        assert_eq!(s.apply_cell(3, 3), Action::Exit);
        // State should be untouched — exit doesn't adjust anything.
        assert_eq!(s, seed());
    }

    #[test]
    fn apply_cell_inert_cells_are_noops() {
        // col 1 on non-exit rows = label/value display cell; col 3 on
        // rows 0..=2 is unused. Both should be no-ops and leave state
        // untouched so a stray finger on an unused cell doesn't drift.
        let mut s = seed();
        let original = s.clone();
        for row in 0..3u8 {
            assert_eq!(s.apply_cell(row, 1), Action::None);
            assert_eq!(s.apply_cell(row, 3), Action::None);
        }
        // row 3 cols 0..=2 are unused (only (3,3) acts).
        for col in 0..3u8 {
            assert_eq!(s.apply_cell(3, col), Action::None);
        }
        assert_eq!(s, original);
    }

    #[test]
    fn from_config_clamps_volume() {
        let cfg = RhythmConfig {
            volume: 2.5, // hand-edited garbage
            ..Default::default()
        };
        let s = SettingsState::from_config(&cfg);
        assert!((s.volume - 1.0).abs() < 1e-9);
    }

    #[test]
    fn write_rhythm_section_creates_section_when_missing() {
        // Starting from a deck.toml with NO [rhythm] section, writing must
        // create one and insert all three player-tunable fields.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("deck.toml");
        std::fs::write(&path, "version = 1\nactive_profile = \"homelab\"\n").unwrap();
        let state = SettingsState {
            audio_offset_ms: -20,
            volume: 0.25,
            sfx_enabled: false,
        };
        write_rhythm_section(&path, &state).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let doc: toml::Value = toml::from_str(&written).unwrap();
        let rhythm = doc.get("rhythm").unwrap().as_table().unwrap();
        assert_eq!(
            rhythm.get("audio_offset_ms").unwrap().as_integer(),
            Some(-20)
        );
        assert!((rhythm.get("volume").unwrap().as_float().unwrap() - 0.25).abs() < 1e-9);
        assert_eq!(rhythm.get("sfx_enabled").unwrap().as_bool(), Some(false));
        // Unrelated keys on the root table must survive.
        assert_eq!(doc.get("active_profile").unwrap().as_str(), Some("homelab"));
    }

    #[test]
    fn write_rhythm_section_preserves_unknown_keys_and_other_sections() {
        // Anything we don't touch — charts_dir (known but not edited here),
        // other top-level sections, forward-compat keys — must round-trip
        // unchanged.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("deck.toml");
        let original = r##"version = 1
active_profile = "homelab"

[editor]
bind = "127.0.0.1:7373"

[rhythm]
charts_dir = "/charts"
audio_offset_ms = 10
volume = 0.8
sfx_enabled = true
mystery_field = "keep me"
"##;
        std::fs::write(&path, original).unwrap();
        let state = SettingsState {
            audio_offset_ms: 42,
            volume: 0.0,
            sfx_enabled: false,
        };
        write_rhythm_section(&path, &state).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let doc: toml::Value = toml::from_str(&written).unwrap();
        let rhythm = doc.get("rhythm").unwrap().as_table().unwrap();
        // Updated fields.
        assert_eq!(
            rhythm.get("audio_offset_ms").unwrap().as_integer(),
            Some(42)
        );
        assert!(rhythm.get("volume").unwrap().as_float().unwrap().abs() < 1e-9);
        assert_eq!(rhythm.get("sfx_enabled").unwrap().as_bool(), Some(false));
        // Unknown field survives untouched.
        assert_eq!(
            rhythm.get("mystery_field").unwrap().as_str(),
            Some("keep me")
        );
        // charts_dir also survives — the settings UI doesn't edit it.
        assert_eq!(rhythm.get("charts_dir").unwrap().as_str(), Some("/charts"));
        // [editor] section untouched.
        let editor = doc.get("editor").unwrap().as_table().unwrap();
        assert_eq!(editor.get("bind").unwrap().as_str(), Some("127.0.0.1:7373"));
    }

    #[test]
    fn exit_writes_expected_toml_end_to_end() {
        // Simulate the full flow: load → mutate via apply_cell → hit exit
        // → write. This is what the run loop does; doing it here end-to-end
        // guards the contract without a winit/GPU context.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("deck.toml");
        std::fs::write(&path, "version = 1\nactive_profile = \"homelab\"\n").unwrap();
        // Seed from the parsed config.
        let text = std::fs::read_to_string(&path).unwrap();
        let cfg: crate::config::DeckConfig = toml::from_str(&text).unwrap();
        let mut state = SettingsState::from_config(&cfg.rhythm);
        // +5 ms offset (one tap), –5% volume (one tap), SFX off (one tap).
        assert_eq!(state.apply_cell(0, 2), Action::Changed);
        assert_eq!(state.apply_cell(1, 0), Action::Changed);
        assert_eq!(state.apply_cell(2, 0), Action::Changed);
        // Exit -> write.
        assert_eq!(state.apply_cell(3, 3), Action::Exit);
        write_rhythm_section(&path, &state).unwrap();
        // Re-parse and confirm.
        let text2 = std::fs::read_to_string(&path).unwrap();
        let cfg2: crate::config::DeckConfig = toml::from_str(&text2).unwrap();
        assert_eq!(cfg2.rhythm.audio_offset_ms, AUDIO_OFFSET_STEP_MS);
        assert!((cfg2.rhythm.volume - (1.0 - VOLUME_STEP)).abs() < 1e-6);
        assert!(!cfg2.rhythm.sfx_enabled);
    }
}
