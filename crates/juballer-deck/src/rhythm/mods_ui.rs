//! In-app gameplay-mods editor. Fullscreen 4×4 grid where each row is
//! one boolean modifier. Col 0 toggles OFF, col 2 toggles ON, col 1 is
//! the label/value, col 3 is unused. Row 3 cell (3,3) = EXIT + save.
//!
//! One mod ships today (`no_fail` on row 0); additional flags append as
//! new rows.

use crate::config::{atomic_write, DeckPaths, ModConfig};
use crate::{Error, Result};
use juballer_core::input::Event;
use juballer_core::{App, Color, Frame, PresentMode};
use juballer_egui::EguiOverlay;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModRow {
    NoFail,
}

impl ModRow {
    fn from_row(row: u8) -> Option<Self> {
        match row {
            0 => Some(Self::NoFail),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Changed,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModsState {
    pub no_fail: bool,
}

impl ModsState {
    pub fn from_config(cfg: &ModConfig) -> Self {
        Self {
            no_fail: cfg.no_fail,
        }
    }

    /// Pure state transition for a cell press. EXIT wins from any state;
    /// col 0 toggles OFF, col 2 toggles ON. Unknown row = no-op.
    pub fn apply_cell(&mut self, row: u8, col: u8) -> Action {
        if row == 3 && col == 3 {
            return Action::Exit;
        }
        let Some(mod_row) = ModRow::from_row(row) else {
            return Action::None;
        };
        let sign: i8 = match col {
            0 => -1,
            2 => 1,
            _ => return Action::None,
        };
        match mod_row {
            ModRow::NoFail => {
                let prev = self.no_fail;
                self.no_fail = sign > 0;
                if self.no_fail == prev {
                    Action::None
                } else {
                    Action::Changed
                }
            }
        }
    }
}

/// Merge mods state into the `[rhythm.mods]` section of deck.toml. Same
/// read → edit → atomic-write pattern as settings_ui so unrelated
/// config keys / sections survive.
fn write_mods_section(deck_path: &Path, state: &ModsState) -> Result<()> {
    let raw = std::fs::read_to_string(deck_path)
        .map_err(|e| Error::Config(format!("mods: read {}: {e}", deck_path.display())))?;
    let mut doc: toml::Value = raw
        .parse()
        .map_err(|e| Error::Config(format!("mods: parse {}: {e}", deck_path.display())))?;
    let root = doc
        .as_table_mut()
        .ok_or_else(|| Error::Config("mods: deck.toml root is not a table".into()))?;
    let rhythm = root
        .entry("rhythm".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let rhythm_tbl = rhythm
        .as_table_mut()
        .ok_or_else(|| Error::Config("mods: [rhythm] is not a table".into()))?;
    let mods = rhythm_tbl
        .entry("mods".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let mods_tbl = mods
        .as_table_mut()
        .ok_or_else(|| Error::Config("mods: [rhythm.mods] is not a table".into()))?;
    mods_tbl.insert("no_fail".into(), toml::Value::Boolean(state.no_fail));
    atomic_write(
        deck_path,
        toml::to_string_pretty(&doc).unwrap_or_default().as_bytes(),
    )
    .map_err(|e| Error::Config(format!("mods: write {}: {e}", deck_path.display())))
}

/// Entry point. Opens a fullscreen app, shows the mods grid, and on
/// (3,3) writes back to `deck.toml` (preserving other config).
pub fn run(paths: &DeckPaths) -> Result<()> {
    let tree = crate::config::ConfigTree::load(paths)?;
    let initial = ModsState::from_config(&tree.deck.rhythm.mods);
    let mut state = initial.clone();

    let mut app = App::builder()
        .title("juballer — mods")
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
                            if let Err(e) = write_mods_section(&deck_path, &state) {
                                tracing::warn!(
                                    target: "juballer::rhythm::mods",
                                    "write failed: {e}"
                                );
                            } else {
                                tracing::info!(
                                    target: "juballer::rhythm::mods",
                                    "mods saved → {}",
                                    deck_path.display()
                                );
                            }
                        }
                        super::exit::exit(0);
                    }
                    Action::Changed | Action::None => {}
                },
                Event::Unmapped { key, .. } if key.0 == "NAMED_Escape" => {
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
            } else if ModRow::from_row(r).is_none() {
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

fn draw_overlay(frame: &mut Frame, overlay: &mut EguiOverlay, state: &ModsState) {
    let cell_rects = *frame.cell_rects();
    overlay.draw(frame, |rc| {
        let rows: [(ModRow, &str, String); 1] = [(
            ModRow::NoFail,
            "NO-FAIL",
            if state.no_fail {
                "ON".into()
            } else {
                "OFF".into()
            },
        )];
        for (row_enum, label, value) in rows.iter() {
            let row_num = match row_enum {
                ModRow::NoFail => 0u8,
            };
            // Col 0 — "–" (off)
            paint_centered(
                rc.ctx(),
                cell_rects[(row_num as usize) * 4],
                "OFF",
                22.0,
                egui::Color32::from_rgb(230, 160, 160),
            );
            // Col 1 — label + current value
            let label_rect = cell_rects[(row_num as usize) * 4 + 1];
            egui::Area::new(egui::Id::new(("mods_label", row_num)))
                .fixed_pos(egui::pos2(label_rect.x as f32, label_rect.y as f32))
                .order(egui::Order::Foreground)
                .show(rc.ctx(), |ui| {
                    ui.set_width(label_rect.w as f32);
                    ui.set_height(label_rect.h as f32);
                    let p = ui.painter();
                    let c = ui.max_rect().center();
                    p.text(
                        c - egui::vec2(0.0, 14.0),
                        egui::Align2::CENTER_CENTER,
                        *label,
                        egui::FontId::proportional(18.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                    p.text(
                        c + egui::vec2(0.0, 14.0),
                        egui::Align2::CENTER_CENTER,
                        value,
                        egui::FontId::proportional(26.0),
                        egui::Color32::WHITE,
                    );
                });
            // Col 2 — "+" (on)
            paint_centered(
                rc.ctx(),
                cell_rects[(row_num as usize) * 4 + 2],
                "ON",
                22.0,
                egui::Color32::from_rgb(160, 230, 180),
            );
        }
        // EXIT cell.
        let exit_rect = cell_rects[15];
        paint_centered(
            rc.ctx(),
            exit_rect,
            "EXIT",
            22.0,
            egui::Color32::from_rgb(240, 120, 130),
        );
    });
}

fn paint_centered(
    ctx: &egui::Context,
    rect: juballer_core::Rect,
    text: &str,
    size: f32,
    color: egui::Color32,
) {
    let id = egui::Id::new(("mods_cell", rect.x, rect.y));
    egui::Area::new(id)
        .fixed_pos(egui::pos2(rect.x as f32, rect.y as f32))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_width(rect.w as f32);
            ui.set_height(rect.h as f32);
            let p = ui.painter();
            let c = ui.max_rect().center();
            p.text(
                c,
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

    #[test]
    fn apply_cell_toggles_no_fail_from_off_to_on() {
        let mut s = ModsState { no_fail: false };
        assert_eq!(s.apply_cell(0, 2), Action::Changed);
        assert!(s.no_fail);
    }

    #[test]
    fn apply_cell_off_is_idempotent_when_already_off() {
        let mut s = ModsState { no_fail: false };
        assert_eq!(s.apply_cell(0, 0), Action::None);
        assert!(!s.no_fail);
    }

    #[test]
    fn apply_cell_exit_wins() {
        let mut s = ModsState { no_fail: false };
        assert_eq!(s.apply_cell(3, 3), Action::Exit);
    }

    #[test]
    fn unknown_row_is_noop() {
        let mut s = ModsState { no_fail: false };
        assert_eq!(s.apply_cell(2, 0), Action::None);
        assert_eq!(s.apply_cell(2, 2), Action::None);
    }
}
