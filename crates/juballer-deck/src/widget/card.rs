//! Shared card container helper.
//!
//! Widgets render into a pane-sized `egui::Ui`. `draw_card` paints a rounded
//! surface + 1px border, an optional header band with title/badge, a 1px divider,
//! and invokes `content` inside a padded inner UI so each widget looks like a
//! dashboard panel instead of a bare text blob.

use crate::theme::{Theme, FONT_SMALL};

/// Outer corner radius (px).
pub const CARD_RADIUS: f32 = 6.0;
/// Inner padding around card content (px).
pub const CARD_PAD: f32 = 12.0;
/// Header band height when a title/badge is present (px).
pub const HEADER_H: f32 = 22.0;
/// Gap between header divider and content (px).
pub const HEADER_GAP: f32 = 6.0;
/// Vertical spacing between elements inside the card content area.
pub const CONTENT_ITEM_SPACING: f32 = 6.0;

/// Paint the card surface + optional header, then run `content` inside the
/// padded inner area.
///
/// `title` is rendered upper-cased in `theme.subtext0` on the left of the
/// header band. `badge` is rendered right-aligned in the same band, in
/// `badge_color` (falls back to `theme.subtext0`).
pub fn draw_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: Option<&str>,
    badge: Option<(&str, egui::Color32)>,
    content: impl FnOnce(&mut egui::Ui),
) {
    let rect = ui.max_rect();
    if rect.width() < 4.0 || rect.height() < 4.0 {
        return;
    }
    let rounding = egui::Rounding::same(CARD_RADIUS);
    let painter = ui.painter();
    painter.rect_filled(rect, rounding, theme.surface0);
    painter.rect_stroke(rect, rounding, egui::Stroke::new(1.0, theme.surface1));

    let has_header = title.is_some() || badge.is_some();
    let content_top = if has_header {
        let header_rect =
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + HEADER_H));
        let header_rounding = egui::Rounding {
            nw: CARD_RADIUS,
            ne: CARD_RADIUS,
            sw: 0.0,
            se: 0.0,
        };
        painter.rect_filled(header_rect, header_rounding, theme.mantle);

        if let Some(t) = title {
            let text = t.to_uppercase();
            painter.text(
                header_rect.left_center() + egui::vec2(CARD_PAD, 0.0),
                egui::Align2::LEFT_CENTER,
                &text,
                egui::FontId::proportional(FONT_SMALL),
                theme.subtext0,
            );
        }
        if let Some((b, color)) = badge {
            painter.text(
                header_rect.right_center() - egui::vec2(CARD_PAD, 0.0),
                egui::Align2::RIGHT_CENTER,
                b,
                egui::FontId::proportional(FONT_SMALL),
                color,
            );
        }

        let divider_y = header_rect.max.y;
        painter.hline(
            rect.min.x..=rect.max.x,
            divider_y,
            egui::Stroke::new(1.0, theme.surface1),
        );
        divider_y + HEADER_GAP
    } else {
        rect.min.y + CARD_PAD
    };

    let inner_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + CARD_PAD, content_top),
        egui::pos2(rect.max.x - CARD_PAD, rect.max.y - CARD_PAD),
    );
    if inner_rect.width() <= 0.0 || inner_rect.height() <= 0.0 {
        return;
    }
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner_rect)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    inner.set_clip_rect(inner_rect);
    inner.spacing_mut().item_spacing.y = CONTENT_ITEM_SPACING;
    inner.spacing_mut().item_spacing.x = 4.0;
    content(&mut inner);
}

/// Horizontal rounded progress bar. Used by widgets that want a consistent bar
/// (e.g. sysinfo). `frac` is clamped to `[0, 1]`.
pub fn progress_bar(
    ui: &mut egui::Ui,
    frac: f32,
    height: f32,
    fill: egui::Color32,
    track: egui::Color32,
) {
    let frac = frac.clamp(0.0, 1.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let radius = (height * 0.5).min(4.0);
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::same(radius), track);
    if frac > 0.0 {
        let mut fg = rect;
        fg.max.x = rect.min.x + rect.width() * frac;
        painter.rect_filled(fg, egui::Rounding::same(radius), fill);
    }
}
