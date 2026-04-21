//! action_mini widget — renders a tile-style button in a top-region pane.
//! Useful for promoting an action into the top region. Fires the action when clicked.
//!
//! Args:
//!   icon  : string (optional) — emoji or asset path (3 chars or fewer = emoji)
//!   label : string (optional)
//!   action: string (required) — action name (must exist in the action registry)
//!   args  : table (optional)

use crate::theme::{ease_out_cubic, FONT_BODY, FONT_SMALL};
use crate::widget::card::CARD_RADIUS;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::time::Instant;

pub struct ActionMiniWidget {
    icon: Option<String>,
    label: Option<String>,
    action_name: String,
    action_args: toml::Table,
    pressed_at: Option<Instant>,
}

impl WidgetBuildFromArgs for ActionMiniWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let action_name = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("action_mini requires action".into()))?
            .to_string();
        let icon = args.get("icon").and_then(|v| v.as_str()).map(String::from);
        let label = args.get("label").and_then(|v| v.as_str()).map(String::from);
        let action_args = args
            .get("args")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        Ok(Self {
            icon,
            label,
            action_name,
            action_args,
            pressed_at: None,
        })
    }
}

impl Widget for ActionMiniWidget {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let rect = ui.max_rect();
        let tile = rect.shrink(3.0);
        let rounding = egui::CornerRadius::same((CARD_RADIUS) as u8);

        let resp = ui.allocate_rect(rect, egui::Sense::click());
        let painter = ui.painter();

        // Shadow.
        for (off, a) in [(1.0, 70u8), (3.0, 30u8)] {
            let s = tile.translate(egui::vec2(0.0, off));
            let c = theme.crust;
            painter.rect_filled(
                s,
                rounding,
                egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a),
            );
        }
        painter.rect_filled(tile, rounding, theme.surface0);
        painter.rect_stroke(
            tile,
            rounding,
            egui::Stroke::new(1.0, theme.surface1),
            egui::StrokeKind::Middle,
        );

        if resp.hovered() {
            let a = theme.accent;
            painter.rect_stroke(
                tile,
                rounding,
                egui::Stroke::new(
                    1.5,
                    egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 180),
                ),
                egui::StrokeKind::Middle,
            );
        }

        // Press flash.
        if let Some(t) = self.pressed_at {
            let ms = t.elapsed().as_millis() as f32;
            if ms < 250.0 {
                let p = ms / 250.0;
                let fade = 1.0 - ease_out_cubic(p);
                let a = theme.accent;
                painter.rect_filled(
                    tile,
                    rounding,
                    egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), (fade * 70.0) as u8),
                );
            } else {
                self.pressed_at = None;
            }
        }

        let mut inner_ui = ui.new_child(egui::UiBuilder::new().max_rect(tile.shrink(8.0)));
        inner_ui.vertical_centered(|ui| {
            if let Some(ic) = &self.icon {
                let h = ui.available_height();
                let size = (h * 0.45).clamp(14.0, 32.0);
                ui.label(
                    egui::RichText::new(ic)
                        .font(egui::FontId::proportional(size))
                        .color(theme.text),
                );
            }
            if let Some(lbl) = &self.label {
                ui.label(
                    egui::RichText::new(lbl)
                        .size(FONT_BODY - 2.0)
                        .color(theme.text),
                );
            } else {
                ui.label(
                    egui::RichText::new(&self.action_name)
                        .size(FONT_SMALL)
                        .color(theme.subtext0),
                );
            }
        });

        if resp.clicked() {
            self.pressed_at = Some(Instant::now());
            cx.bus.publish(
                "widget.action_request",
                serde_json::json!({
                    "action": &self.action_name,
                    "args": serde_json::to_value(&self.action_args).unwrap_or(serde_json::json!({})),
                }),
            );
        }
        self.pressed_at.is_some() || resp.hovered()
    }
}
