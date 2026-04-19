//! egui painter helpers for outlined text, used to keep overlay text
//! legible against arbitrary banner / jacket backgrounds. The outline is
//! produced by drawing the glyph eight times in black at ±1 pixel
//! offsets before painting the foreground color on top.

/// Draw `text` at `pos` with an eight-direction 1px black outline for
/// legibility over arbitrary backgrounds.
pub fn text_outlined(
    painter: &egui::Painter,
    pos: egui::Pos2,
    anchor: egui::Align2,
    text: impl ToString,
    font_id: egui::FontId,
    fg: egui::Color32,
) {
    const OFFSETS: &[(f32, f32)] = &[
        (-1.0, -1.0),
        (-1.0, 0.0),
        (-1.0, 1.0),
        (0.0, -1.0),
        (0.0, 1.0),
        (1.0, -1.0),
        (1.0, 0.0),
        (1.0, 1.0),
    ];
    let text_str = text.to_string();
    // Slightly translucent black: pure #000 reads as a hard seam against
    // low-contrast backgrounds in the dark theme.
    let outline = egui::Color32::from_black_alpha(230);
    for (dx, dy) in OFFSETS {
        painter.text(
            egui::pos2(pos.x + dx, pos.y + dy),
            anchor,
            &text_str,
            font_id.clone(),
            outline,
        );
    }
    painter.text(pos, anchor, text_str, font_id, fg);
}
