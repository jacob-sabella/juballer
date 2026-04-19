//! clock widget — renders formatted local time inside a card.
//!
//! Args:
//!   format : string (default "%H:%M:%S")  -- chrono strftime format

use crate::theme::{FONT_BODY, FONT_HEADER, FONT_LARGE};
use crate::widget::card::draw_card;
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
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let now = Local::now();
        let time_str = now.format(&self.format).to_string();
        let date_str = now.format("%a %d %b").to_string();
        // Adapt to very short panes by collapsing to a single row and scaling the
        // time down if there isn't room for the hero + date stack.
        let pane_h = ui.max_rect().height();
        let compact = pane_h < 64.0;
        if compact {
            draw_card(ui, &theme, None, None, |ui| {
                ui.label(
                    egui::RichText::new(&time_str)
                        .size(FONT_HEADER)
                        .strong()
                        .color(theme.text),
                );
            });
        } else {
            draw_card(ui, &theme, None, None, |ui| {
                ui.label(
                    egui::RichText::new(&time_str)
                        .size(FONT_LARGE)
                        .strong()
                        .color(theme.text),
                );
                ui.label(
                    egui::RichText::new(&date_str)
                        .size(FONT_BODY)
                        .color(theme.subtext1),
                );
            });
        }
        true
    }
}
