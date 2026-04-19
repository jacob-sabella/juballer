//! Cell painting + top-region HUD for Carla mode.
//!
//! Phase 1 ships a deliberately plain look — every binding type gets a
//! distinct flat colour and a centred label so the user can read what
//! each cell does at a glance. Phase 5 will tighten the visual pass
//! (parameter-value bars, display widgets, animation).

use crate::carla::config::{ActionMode, Cell, DisplayBinding, DisplayMode, PluginRef};
use crate::carla::listener::CarlaFeed;
use crate::carla::picker::{
    self, PickerState, NAV_BACK_COL, NAV_EXIT_COL as PICKER_NAV_EXIT_COL,
    NAV_NEXT_COL as PICKER_NAV_NEXT_COL, NAV_PREV_COL as PICKER_NAV_PREV_COL,
    NAV_ROW as PICKER_NAV_ROW, TILES_PER_PAGE,
};
use crate::carla::preset::PresetEntry;
use crate::carla::preset_picker::PresetPickerState;
use crate::carla::state::CarlaState;
use juballer_core::{Color, Frame};
use juballer_egui::EguiOverlay;
use std::sync::{Arc, RwLock};

/// Bottom-row navigation cells. Layout fixed for the whole carla mode.
pub const NAV_PREV_COL: u8 = 0;
pub const NAV_NEXT_COL: u8 = 1;
pub const NAV_PICKER_COL: u8 = 2;
pub const NAV_EXIT_COL: u8 = 3;
pub const NAV_ROW: u8 = 3;

const PALETTE_EMPTY: Color = Color(0x0a, 0x0c, 0x12, 0xff);
const PALETTE_TAP: Color = Color(0x12, 0x2c, 0x40, 0xff);
const PALETTE_HOLD: Color = Color(0x2a, 0x14, 0x38, 0xff);
const PALETTE_TAP_HOLD: Color = Color(0x18, 0x36, 0x44, 0xff);
const PALETTE_DISPLAY: Color = Color(0x10, 0x24, 0x18, 0xff);
const PALETTE_PRESET: Color = Color(0x36, 0x28, 0x10, 0xff);
const PALETTE_NAV_ACTIVE: Color = Color(0x18, 0x1f, 0x2c, 0xff);
const PALETTE_NAV_DISABLED: Color = Color(0x0a, 0x0a, 0x10, 0xff);
const PALETTE_NAV_EXIT: Color = Color(0x40, 0x10, 0x14, 0xff);
const PALETTE_PICKER_TILE: Color = Color(0x16, 0x1f, 0x2e, 0xff);
const PALETTE_PICKER_EMPTY: Color = Color(0x06, 0x08, 0x0d, 0xff);

/// Paint flat-colour backgrounds for every cell on the active page +
/// the four bottom-row nav cells. Called every frame from
/// [`super::run`].
pub fn paint_backgrounds(frame: &mut Frame<'_>, state: &CarlaState) {
    // Default everything to "empty"; per-cell overrides come next.
    for r in 0..4u8 {
        for c in 0..4u8 {
            frame.grid_cell(r, c).fill(PALETTE_EMPTY);
        }
    }

    if let Some(page) = state.active_page() {
        for cell in &page.cells {
            if cell.row >= NAV_ROW {
                continue; // nav row reserved
            }
            frame.grid_cell(cell.row, cell.col).fill(cell_colour(cell));
        }
    }

    paint_nav_row(frame, state);
}

fn cell_colour(cell: &Cell) -> Color {
    if cell.is_blank() {
        return PALETTE_EMPTY;
    }
    let any_preset = cell
        .tap
        .as_ref()
        .map(|a| a.mode.is_preset())
        .unwrap_or(false)
        || cell
            .hold
            .as_ref()
            .map(|a| a.mode.is_preset())
            .unwrap_or(false);
    if any_preset {
        return PALETTE_PRESET;
    }
    match (
        cell.tap.is_some(),
        cell.hold.is_some(),
        cell.display.is_some(),
    ) {
        (true, true, _) => PALETTE_TAP_HOLD,
        (true, false, _) => PALETTE_TAP,
        (false, true, _) => PALETTE_HOLD,
        (false, false, true) => PALETTE_DISPLAY,
        (false, false, false) => PALETTE_EMPTY,
    }
}

fn paint_nav_row(frame: &mut Frame<'_>, state: &CarlaState) {
    let multi_page = state.page_count() > 1;
    let prev_col = if multi_page {
        PALETTE_NAV_ACTIVE
    } else {
        PALETTE_NAV_DISABLED
    };
    let next_col = prev_col;
    frame.grid_cell(NAV_ROW, NAV_PREV_COL).fill(prev_col);
    frame.grid_cell(NAV_ROW, NAV_NEXT_COL).fill(next_col);
    frame
        .grid_cell(NAV_ROW, NAV_PICKER_COL)
        .fill(PALETTE_NAV_ACTIVE);
    frame
        .grid_cell(NAV_ROW, NAV_EXIT_COL)
        .fill(PALETTE_NAV_EXIT);
}

/// Top-region HUD + per-cell labels. Renders inside the existing egui
/// overlay scaffolding; the painter draws on top of the wgpu cell
/// colours from [`paint_backgrounds`]. When `live_feed` is `Some`,
/// display cells render their value snapshot from the live Carla feed;
/// when `None` they fall back to a placeholder.
pub fn draw_overlay(
    frame: &mut Frame<'_>,
    overlay: &mut EguiOverlay,
    state: &CarlaState,
    live_feed: Option<&Arc<RwLock<CarlaFeed>>>,
) {
    let cell_rects = *frame.cell_rects();
    let top_rect = frame.top_region_rect();

    let config_name = state.config().display_name().to_string();
    let page_idx = state.current_page_index();
    let page_count = state.page_count();
    let page_title = state
        .active_page()
        .and_then(|p| p.title.clone())
        .unwrap_or_default();
    let breadcrumb = state.last_touched().map(|lt| {
        format!(
            "{} · {} = {:.3}",
            display_ref(&lt.plugin),
            display_ref(&lt.param),
            lt.value
        )
    });

    let active_cells: Vec<Cell> = state
        .active_page()
        .map(|p| p.cells.clone())
        .unwrap_or_default();

    let live_status = match live_feed {
        Some(feed) => match feed.read() {
            Ok(g) if g.seen_first_message => Some("● LIVE"),
            Ok(_) => Some("○ waiting…"),
            Err(_) => Some("⚠ feed lock poisoned"),
        },
        None => None,
    };

    // Snapshot the live feed once per frame so every display cell
    // sees consistent values across the same paint pass.
    let feed_snapshot = live_feed.and_then(|f| {
        f.read().ok().map(|g| FeedSnapshot {
            params: g.params.clone(),
            peaks: g.peaks.clone(),
            seen: g.seen_first_message,
        })
    });

    overlay.draw(frame, |rc| {
        draw_top_hud(
            rc.ctx(),
            top_rect,
            &config_name,
            page_idx,
            page_count,
            &page_title,
            breadcrumb.as_deref(),
            live_status,
        );
        for cell in &active_cells {
            if cell.row >= NAV_ROW {
                continue;
            }
            let idx = cell.row as usize * 4 + cell.col as usize;
            let rect = cell_rects[idx];
            let label = cell_label(cell);
            draw_cell_label(rc.ctx(), rect, &label);
            if let (Some(disp), Some(feed)) = (cell.display.as_ref(), feed_snapshot.as_ref()) {
                draw_display(rc.ctx(), rect, disp, feed);
            }
        }
        // Nav-row labels.
        let prev_rect = cell_rects[NAV_ROW as usize * 4 + NAV_PREV_COL as usize];
        let next_rect = cell_rects[NAV_ROW as usize * 4 + NAV_NEXT_COL as usize];
        let pick_rect = cell_rects[NAV_ROW as usize * 4 + NAV_PICKER_COL as usize];
        let exit_rect = cell_rects[NAV_ROW as usize * 4 + NAV_EXIT_COL as usize];
        draw_cell_label(rc.ctx(), prev_rect, "◀ PAGE");
        draw_cell_label(rc.ctx(), next_rect, "PAGE ▶");
        draw_cell_label(rc.ctx(), pick_rect, "CONFIGS");
        draw_cell_label(rc.ctx(), exit_rect, "EXIT");
    });
}

/// Frame-local copy of the live feed so the overlay closure can read
/// values without holding the RwLock across each cell draw.
struct FeedSnapshot {
    params: std::collections::HashMap<(u32, u32), f32>,
    peaks: std::collections::HashMap<u32, [f32; 4]>,
    seen: bool,
}

impl FeedSnapshot {
    fn param(&self, plugin: u32, param: u32) -> Option<f32> {
        self.params.get(&(plugin, param)).copied()
    }
    fn peaks(&self, plugin: u32) -> Option<&[f32; 4]> {
        self.peaks.get(&plugin)
    }
}

fn draw_display(
    ctx: &egui::Context,
    rect: juballer_core::Rect,
    binding: &DisplayBinding,
    feed: &FeedSnapshot,
) {
    let plugin = match binding.plugin.as_ref().and_then(plugin_ref_index) {
        Some(p) => p,
        None => return,
    };
    let waiting = !feed.seen;
    let id = egui::Id::new(("carla_display", rect.x, rect.y));
    egui::Area::new(id)
        .fixed_pos(egui::pos2(
            rect.x as f32,
            rect.y as f32 + rect.h as f32 * 0.5,
        ))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_width(rect.w as f32);
            ui.set_height(rect.h as f32 * 0.5);
            let painter = ui.painter();
            let center = ui.max_rect().center();
            let (text, color) = match binding.mode {
                DisplayMode::Tuner => render_tuner(binding, plugin, feed, waiting),
                DisplayMode::Meter => render_meter(binding, plugin, feed, waiting, painter, &rect),
                DisplayMode::Value => render_value(binding, plugin, feed, waiting),
                DisplayMode::Text => render_value(binding, plugin, feed, waiting),
                DisplayMode::ActivePresetName => (
                    "preset…".to_string(),
                    egui::Color32::from_rgb(140, 140, 160),
                ),
            };
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::monospace(15.0),
                color,
            );
        });
}

fn render_value(
    binding: &DisplayBinding,
    plugin: u32,
    feed: &FeedSnapshot,
    waiting: bool,
) -> (String, egui::Color32) {
    let param = match binding.source_param.as_ref().and_then(plugin_ref_index) {
        Some(p) => p,
        None => return ("—".into(), placeholder_colour()),
    };
    match feed.param(plugin, param) {
        Some(v) => match &binding.format {
            Some(fmt) => (apply_format(fmt, v), value_colour()),
            None => (format!("{v:.3}"), value_colour()),
        },
        None if waiting => ("connecting…".into(), placeholder_colour()),
        None => ("—".into(), placeholder_colour()),
    }
}

fn render_tuner(
    binding: &DisplayBinding,
    plugin: u32,
    feed: &FeedSnapshot,
    waiting: bool,
) -> (String, egui::Color32) {
    let freq_idx = match binding.freq_param.as_ref().and_then(plugin_ref_index) {
        Some(p) => p,
        None => return ("—".into(), placeholder_colour()),
    };
    let freq = match feed.param(plugin, freq_idx) {
        Some(f) => f,
        None if waiting => return ("connecting…".into(), placeholder_colour()),
        None => return ("—".into(), placeholder_colour()),
    };
    if freq <= 0.0 {
        return ("listening".into(), placeholder_colour());
    }
    let (note, cents) = note_and_cents(freq);
    let arrow = if cents.abs() < 5.0 {
        "·"
    } else if cents > 0.0 {
        "↑"
    } else {
        "↓"
    };
    let colour = if cents.abs() < 5.0 {
        egui::Color32::from_rgb(120, 220, 130)
    } else if cents.abs() < 20.0 {
        egui::Color32::from_rgb(220, 200, 130)
    } else {
        egui::Color32::from_rgb(220, 130, 130)
    };
    (format!("{note} {arrow}{:+.0}¢", cents), colour)
}

fn render_meter(
    binding: &DisplayBinding,
    plugin: u32,
    feed: &FeedSnapshot,
    waiting: bool,
    painter: &egui::Painter,
    rect: &juballer_core::Rect,
) -> (String, egui::Color32) {
    let value = match binding
        .source_param
        .as_ref()
        .and_then(plugin_ref_index)
        .and_then(|p| feed.param(plugin, p))
    {
        Some(v) => Some(v),
        None => feed.peaks(plugin).map(|p| p[2].max(p[3])),
    };
    let value = match value {
        Some(v) => v,
        None if waiting => return ("connecting…".into(), placeholder_colour()),
        None => return ("—".into(), placeholder_colour()),
    };
    // Draw a horizontal bar across the full rect at ~60% height.
    let level = value.clamp(0.0, 1.0);
    let bar_h = (rect.h as f32) * 0.18;
    let bar_y = rect.y as f32 + rect.h as f32 * 0.62;
    let bar_left = rect.x as f32 + 8.0;
    let bar_right = rect.x as f32 + rect.w as f32 - 8.0;
    let total_w = bar_right - bar_left;
    let fill_w = total_w * level;
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(bar_left, bar_y),
            egui::pos2(bar_left + fill_w, bar_y + bar_h),
        ),
        2.0,
        meter_colour(level),
    );
    painter.rect_stroke(
        egui::Rect::from_min_max(
            egui::pos2(bar_left, bar_y),
            egui::pos2(bar_right, bar_y + bar_h),
        ),
        2.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 80, 100)),
    );
    (format!("{:.2}", value), value_colour())
}

fn meter_colour(level: f32) -> egui::Color32 {
    if level > 0.95 {
        egui::Color32::from_rgb(220, 80, 80)
    } else if level > 0.7 {
        egui::Color32::from_rgb(220, 200, 100)
    } else {
        egui::Color32::from_rgb(110, 200, 130)
    }
}

fn placeholder_colour() -> egui::Color32 {
    egui::Color32::from_rgb(100, 110, 130)
}

fn value_colour() -> egui::Color32 {
    egui::Color32::from_rgb(220, 220, 230)
}

fn apply_format(fmt: &str, value: f32) -> String {
    if fmt.contains("{}") {
        fmt.replace("{}", &format!("{value}"))
    } else if fmt.contains("{value}") {
        fmt.replace("{value}", &format!("{value}"))
    } else {
        format!("{fmt} {value}")
    }
}

fn plugin_ref_index(r: &PluginRef) -> Option<u32> {
    match r {
        PluginRef::Index(i) => Some(*i),
        PluginRef::Name(_) => None,
    }
}

/// Convert a frequency in Hz to (note name, cents from nearest semitone).
/// Reference pitch A4 = 440 Hz. Note names use sharps; cents range
/// roughly -50..=+50.
fn note_and_cents(freq: f32) -> (String, f32) {
    if freq <= 0.0 {
        return ("—".into(), 0.0);
    }
    // MIDI note number, fractional. A4 = 69, 440 Hz.
    let midi = 69.0 + 12.0 * (freq / 440.0).log2();
    let nearest = midi.round();
    let cents = (midi - nearest) * 100.0;
    let names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let n = nearest as i32;
    let octave = (n / 12) - 1;
    let idx = (((n % 12) + 12) % 12) as usize;
    (format!("{}{}", names[idx], octave), cents)
}

/// Picker overlay backgrounds. Tiles 0..=11 use a uniform tone; row
/// 3 carries the picker's own nav layout (PREV / NEXT / BACK / EXIT).
pub fn paint_picker(frame: &mut Frame<'_>, picker: &PickerState) {
    for r in 0..4u8 {
        for c in 0..4u8 {
            frame.grid_cell(r, c).fill(PALETTE_PICKER_EMPTY);
        }
    }
    let entries = picker.current_entries();
    for (i, _entry) in entries.iter().enumerate().take(TILES_PER_PAGE) {
        let r = (i / 4) as u8;
        let c = (i % 4) as u8;
        frame.grid_cell(r, c).fill(PALETTE_PICKER_TILE);
    }
    let multi_page = picker.page_count() > 1;
    let nav = if multi_page {
        PALETTE_NAV_ACTIVE
    } else {
        PALETTE_NAV_DISABLED
    };
    frame
        .grid_cell(PICKER_NAV_ROW, PICKER_NAV_PREV_COL)
        .fill(nav);
    frame
        .grid_cell(PICKER_NAV_ROW, PICKER_NAV_NEXT_COL)
        .fill(nav);
    frame
        .grid_cell(PICKER_NAV_ROW, NAV_BACK_COL)
        .fill(PALETTE_NAV_ACTIVE);
    frame
        .grid_cell(PICKER_NAV_ROW, PICKER_NAV_EXIT_COL)
        .fill(PALETTE_NAV_EXIT);
}

/// Preset picker background. Same layout as the config picker but
/// with a distinct amber tile palette so the two overlays are
/// visually distinguishable.
pub fn paint_preset_picker(frame: &mut Frame<'_>, picker: &PresetPickerState) {
    for r in 0..4u8 {
        for c in 0..4u8 {
            frame.grid_cell(r, c).fill(PALETTE_PICKER_EMPTY);
        }
    }
    let entries = picker.current_entries();
    for (i, _entry) in entries.iter().enumerate().take(TILES_PER_PAGE) {
        let r = (i / 4) as u8;
        let c = (i % 4) as u8;
        frame.grid_cell(r, c).fill(PALETTE_PRESET);
    }
    let multi_page = picker.page_count() > 1;
    let nav = if multi_page {
        PALETTE_NAV_ACTIVE
    } else {
        PALETTE_NAV_DISABLED
    };
    frame
        .grid_cell(PICKER_NAV_ROW, PICKER_NAV_PREV_COL)
        .fill(nav);
    frame
        .grid_cell(PICKER_NAV_ROW, PICKER_NAV_NEXT_COL)
        .fill(nav);
    frame
        .grid_cell(PICKER_NAV_ROW, NAV_BACK_COL)
        .fill(PALETTE_NAV_ACTIVE);
    frame
        .grid_cell(PICKER_NAV_ROW, PICKER_NAV_EXIT_COL)
        .fill(PALETTE_NAV_EXIT);
}

/// Preset picker overlay labels + top-region HUD.
pub fn draw_preset_picker_overlay(
    frame: &mut Frame<'_>,
    overlay: &mut EguiOverlay,
    picker: &PresetPickerState,
) {
    let cell_rects = *frame.cell_rects();
    let top_rect = frame.top_region_rect();
    let header = match picker.category() {
        Some(cat) => format!(
            "presets · {cat}    page {}/{}    {} total    ← target {}",
            picker.current_page_index() + 1,
            picker.page_count(),
            picker.total(),
            picker
                .target_plugin()
                .map(|p| format!("plugin #{p}"))
                .unwrap_or_else(|| "no plugin".into())
        ),
        None => format!(
            "presets    page {}/{}    {} total    ← target {}",
            picker.current_page_index() + 1,
            picker.page_count(),
            picker.total(),
            picker
                .target_plugin()
                .map(|p| format!("plugin #{p}"))
                .unwrap_or_else(|| "no plugin".into())
        ),
    };
    let entries: Vec<PresetEntry> = picker.current_entries().to_vec();

    overlay.draw(frame, |rc| {
        draw_top_hud(rc.ctx(), top_rect, &header, 0, 0, "", None, None);
        for (i, entry) in entries.iter().enumerate().take(TILES_PER_PAGE) {
            let rect = cell_rects[i];
            let label = match &entry.preset.description {
                Some(desc) if !desc.is_empty() => format!("{}\n{desc}", entry.name()),
                _ => entry.name(),
            };
            draw_cell_label(rc.ctx(), rect, &label);
        }
        let prev_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + PICKER_NAV_PREV_COL as usize];
        let next_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + PICKER_NAV_NEXT_COL as usize];
        let back_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + NAV_BACK_COL as usize];
        let exit_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + PICKER_NAV_EXIT_COL as usize];
        draw_cell_label(rc.ctx(), prev_rect, "◀ PAGE");
        draw_cell_label(rc.ctx(), next_rect, "PAGE ▶");
        draw_cell_label(rc.ctx(), back_rect, "BACK");
        draw_cell_label(rc.ctx(), exit_rect, "EXIT");
    });
}

/// Picker overlay labels. One tile per visible config (name + optional
/// description), plus the bottom-row nav glyphs.
pub fn draw_picker_overlay(frame: &mut Frame<'_>, overlay: &mut EguiOverlay, picker: &PickerState) {
    let cell_rects = *frame.cell_rects();
    let top_rect = frame.top_region_rect();
    let header = format!(
        "configs    page {}/{}    {} total",
        picker.current_page_index() + 1,
        picker.page_count(),
        picker.total(),
    );
    let entries: Vec<picker::ConfigEntry> = picker.current_entries().to_vec();

    overlay.draw(frame, |rc| {
        draw_top_hud(rc.ctx(), top_rect, &header, 0, 0, "", None, None);
        for (i, entry) in entries.iter().enumerate().take(TILES_PER_PAGE) {
            let rect = cell_rects[i];
            let label = match &entry.description {
                Some(desc) if !desc.is_empty() => format!("{}\n{}", entry.name, desc),
                _ => entry.name.clone(),
            };
            draw_cell_label(rc.ctx(), rect, &label);
        }
        let prev_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + PICKER_NAV_PREV_COL as usize];
        let next_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + PICKER_NAV_NEXT_COL as usize];
        let back_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + NAV_BACK_COL as usize];
        let exit_rect = cell_rects[PICKER_NAV_ROW as usize * 4 + PICKER_NAV_EXIT_COL as usize];
        draw_cell_label(rc.ctx(), prev_rect, "◀ PAGE");
        draw_cell_label(rc.ctx(), next_rect, "PAGE ▶");
        draw_cell_label(rc.ctx(), back_rect, "BACK");
        draw_cell_label(rc.ctx(), exit_rect, "EXIT");
    });
}

fn cell_label(cell: &Cell) -> String {
    if let Some(label) = &cell.label {
        return label.clone();
    }
    let mut parts = Vec::new();
    if let Some(action) = &cell.tap {
        parts.push(short_action(action.mode));
    }
    if let Some(action) = &cell.hold {
        parts.push(format!("hold:{}", short_action(action.mode)));
    }
    if let Some(display) = &cell.display {
        parts.push(format!("disp:{}", short_display(display.mode)));
    }
    if parts.is_empty() {
        return String::new();
    }
    parts.join("\n")
}

fn short_action(mode: ActionMode) -> String {
    match mode {
        ActionMode::BumpUp => "+",
        ActionMode::BumpDown => "−",
        ActionMode::Toggle => "⏻",
        ActionMode::Momentary => "●",
        ActionMode::Set => "≡",
        ActionMode::CarouselNext => "▶",
        ActionMode::CarouselPrev => "◀",
        ActionMode::LoadPreset => "★ load",
        ActionMode::OpenPresetPicker => "★ pick",
    }
    .to_string()
}

fn short_display(mode: DisplayMode) -> &'static str {
    match mode {
        DisplayMode::Tuner => "tune",
        DisplayMode::Meter => "meter",
        DisplayMode::Value => "val",
        DisplayMode::Text => "text",
        DisplayMode::ActivePresetName => "preset",
    }
}

fn display_ref(r: &PluginRef) -> String {
    match r {
        PluginRef::Index(i) => format!("#{i}"),
        PluginRef::Name(n) => n.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_top_hud(
    ctx: &egui::Context,
    rect: juballer_core::Rect,
    config_name: &str,
    page_idx: usize,
    page_count: usize,
    page_title: &str,
    breadcrumb: Option<&str>,
    live_status: Option<&str>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    let line = if page_title.is_empty() {
        format!("{config_name}    page {}/{page_count}", page_idx + 1)
    } else {
        format!(
            "{config_name}    {}    page {}/{page_count}",
            page_title,
            page_idx + 1,
        )
    };
    egui::Area::new(egui::Id::new("carla_hud"))
        .fixed_pos(egui::pos2(rect.x as f32, rect.y as f32))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_width(rect.w as f32);
            ui.set_height(rect.h as f32);
            let painter = ui.painter();
            let title_pos = egui::pos2(
                rect.x as f32 + 16.0,
                rect.y as f32 + 16.0 + rect.h as f32 * 0.10,
            );
            painter.text(
                title_pos,
                egui::Align2::LEFT_TOP,
                line,
                egui::FontId::proportional(20.0),
                egui::Color32::from_rgb(220, 220, 230),
            );
            if let Some(crumb) = breadcrumb {
                let crumb_pos =
                    egui::pos2(rect.x as f32 + 16.0, rect.y as f32 + rect.h as f32 - 26.0);
                painter.text(
                    crumb_pos,
                    egui::Align2::LEFT_TOP,
                    crumb,
                    egui::FontId::monospace(14.0),
                    egui::Color32::from_rgb(150, 200, 220),
                );
            }
            if let Some(status) = live_status {
                let status_pos =
                    egui::pos2(rect.x as f32 + rect.w as f32 - 120.0, rect.y as f32 + 18.0);
                let colour = if status.starts_with('●') {
                    egui::Color32::from_rgb(110, 220, 130)
                } else {
                    egui::Color32::from_rgb(200, 180, 110)
                };
                painter.text(
                    status_pos,
                    egui::Align2::LEFT_TOP,
                    status,
                    egui::FontId::monospace(13.0),
                    colour,
                );
            }
        });
}

fn draw_cell_label(ctx: &egui::Context, rect: juballer_core::Rect, text: &str) {
    if text.is_empty() || rect.w == 0 {
        return;
    }
    let id = egui::Id::new(("carla_cell", rect.x, rect.y));
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
                egui::FontId::proportional(18.0),
                egui::Color32::from_rgb(220, 220, 230),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carla::config::{Action, Configuration, Page, PluginRef};

    fn cell_with(action: Option<Action>, hold: Option<Action>) -> Cell {
        Cell {
            row: 0,
            col: 0,
            label: None,
            tap: action,
            hold,
            display: None,
        }
    }

    fn act(mode: ActionMode) -> Action {
        Action {
            plugin: PluginRef::Index(0),
            param: Some(PluginRef::Index(0)),
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

    #[test]
    fn cell_colour_picks_palette_per_slot_combination() {
        assert_eq!(cell_colour(&cell_with(None, None)), PALETTE_EMPTY);
        assert_eq!(
            cell_colour(&cell_with(Some(act(ActionMode::Set)), None)),
            PALETTE_TAP
        );
        assert_eq!(
            cell_colour(&cell_with(None, Some(act(ActionMode::Set)))),
            PALETTE_HOLD
        );
        assert_eq!(
            cell_colour(&cell_with(
                Some(act(ActionMode::Set)),
                Some(act(ActionMode::Set))
            )),
            PALETTE_TAP_HOLD
        );
    }

    #[test]
    fn cell_colour_marks_preset_modes_amber() {
        let c = cell_with(Some(act(ActionMode::LoadPreset)), None);
        assert_eq!(cell_colour(&c), PALETTE_PRESET);
    }

    #[test]
    fn cell_label_falls_back_to_mode_glyph_when_no_label_set() {
        let c = cell_with(Some(act(ActionMode::BumpUp)), None);
        assert_eq!(cell_label(&c), "+");
    }

    #[test]
    fn cell_label_uses_user_override_when_present() {
        let mut c = cell_with(Some(act(ActionMode::BumpUp)), None);
        c.label = Some("Wet+".into());
        assert_eq!(cell_label(&c), "Wet+");
    }

    #[test]
    fn cell_label_combines_tap_and_hold_modes_on_separate_lines() {
        let c = cell_with(Some(act(ActionMode::BumpUp)), Some(act(ActionMode::Set)));
        let label = cell_label(&c);
        assert!(label.contains('+'));
        assert!(label.contains("hold:≡"));
    }

    #[test]
    fn display_ref_formats_index_with_hash_prefix_and_name_verbatim() {
        assert_eq!(display_ref(&PluginRef::Index(7)), "#7");
        assert_eq!(display_ref(&PluginRef::Name("Roomy".into())), "Roomy");
    }

    #[test]
    fn nav_row_constants_match_documented_layout() {
        // Locking the layout in via constants — tests guard against
        // accidental shuffles when the rest of carla evolves.
        assert_eq!((NAV_ROW, NAV_PREV_COL), (3, 0));
        assert_eq!((NAV_ROW, NAV_NEXT_COL), (3, 1));
        assert_eq!((NAV_ROW, NAV_PICKER_COL), (3, 2));
        assert_eq!((NAV_ROW, NAV_EXIT_COL), (3, 3));
    }

    #[test]
    fn cell_label_is_empty_for_blank_cell() {
        let c = cell_with(None, None);
        assert!(cell_label(&c).is_empty());
    }

    #[test]
    fn note_and_cents_recognises_concert_a_at_440hz_with_zero_cents() {
        let (note, cents) = note_and_cents(440.0);
        assert_eq!(note, "A4");
        assert!(cents.abs() < 0.01, "concert A should be exactly 0 cents");
    }

    #[test]
    fn note_and_cents_climbs_an_octave_per_doubling() {
        assert_eq!(note_and_cents(880.0).0, "A5");
        assert_eq!(note_and_cents(220.0).0, "A3");
    }

    #[test]
    fn note_and_cents_reports_sharp_quarter_tone_above_a4() {
        // Half a semitone above A4 ≈ 452.9 Hz — should land on A4 still
        // with a +50 cents reading.
        let (note, cents) = note_and_cents(452.89);
        assert_eq!(note, "A4");
        assert!((cents - 50.0).abs() < 1.0, "expected ~+50¢, got {cents}");
    }

    #[test]
    fn note_and_cents_resolves_middle_c_within_a_cent() {
        let (note, cents) = note_and_cents(261.6256);
        assert_eq!(note, "C4");
        assert!(cents.abs() < 1.0, "C4 ref freq should be ~0¢ (got {cents})");
    }

    #[test]
    fn apply_format_supports_braces_and_named_substitution() {
        assert_eq!(apply_format("{}", 1.0), "1");
        assert_eq!(apply_format("Wet: {value}", 0.5), "Wet: 0.5");
        // Unrecognised template appends the value verbatim.
        assert_eq!(apply_format("dB", -3.0), "dB -3");
    }

    // Smoke-test that Configuration is reachable from this module so
    // the pub(crate) visibility for renderer helpers stays compatible
    // with the rest of the carla module tree.
    #[test]
    fn render_module_can_observe_a_minimal_configuration() {
        let cfg = Configuration {
            name: Some("X".into()),
            description: None,
            carla: Default::default(),
            pages: vec![Page {
                title: Some("P".into()),
                cells: vec![],
            }],
        };
        assert_eq!(cfg.display_name(), "X");
    }
}
