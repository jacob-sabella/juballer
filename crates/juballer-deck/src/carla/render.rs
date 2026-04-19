//! Cell painting + top-region HUD for Carla mode.
//!
//! Phase 1 ships a deliberately plain look — every binding type gets a
//! distinct flat colour and a centred label so the user can read what
//! each cell does at a glance. Phase 5 will tighten the visual pass
//! (parameter-value bars, display widgets, animation).

use crate::carla::config::{ActionMode, Cell, DisplayMode, PluginRef};
use crate::carla::state::CarlaState;
use juballer_core::{Color, Frame};
use juballer_egui::EguiOverlay;

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
/// colours from [`paint_backgrounds`].
pub fn draw_overlay(frame: &mut Frame<'_>, overlay: &mut EguiOverlay, state: &CarlaState) {
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

    overlay.draw(frame, |rc| {
        draw_top_hud(
            rc.ctx(),
            top_rect,
            &config_name,
            page_idx,
            page_count,
            &page_title,
            breadcrumb.as_deref(),
        );
        for cell in &active_cells {
            if cell.row >= NAV_ROW {
                continue;
            }
            let idx = cell.row as usize * 4 + cell.col as usize;
            let rect = cell_rects[idx];
            let label = cell_label(cell);
            draw_cell_label(rc.ctx(), rect, &label);
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

fn draw_top_hud(
    ctx: &egui::Context,
    rect: juballer_core::Rect,
    config_name: &str,
    page_idx: usize,
    page_count: usize,
    page_title: &str,
    breadcrumb: Option<&str>,
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
