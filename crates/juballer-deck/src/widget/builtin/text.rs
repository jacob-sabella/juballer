//! text widget — static text, optionally wrapped in a card when `title` is given.
//!
//! Args:
//!   content : string (required) — the text to render
//!   size    : string (optional, "small"|"body"|"heading", default "body")
//!   title   : string (optional) — when present, wraps the text in a card with that header

use crate::theme::{FONT_BODY, FONT_HEADER, FONT_SMALL};
use crate::widget::card::draw_card;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};

pub enum TextSize {
    Small,
    Body,
    Heading,
}

pub struct Text {
    content: String,
    size: TextSize,
    title: Option<String>,
}

impl WidgetBuildFromArgs for Text {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("text widget requires args.content (string)".into()))?
            .to_string();
        let size = match args.get("size").and_then(|v| v.as_str()).unwrap_or("body") {
            "small" => TextSize::Small,
            "heading" => TextSize::Heading,
            _ => TextSize::Body,
        };
        let title = args.get("title").and_then(|v| v.as_str()).map(String::from);
        Ok(Self {
            content,
            size,
            title,
        })
    }
}

impl Widget for Text {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let (size, color) = match self.size {
            TextSize::Small => (FONT_SMALL, theme.subtext1),
            TextSize::Body => (FONT_BODY, theme.text),
            TextSize::Heading => (FONT_HEADER, theme.text),
        };
        let content = self.content.clone();
        match &self.title {
            Some(title) => {
                draw_card(ui, &theme, Some(title), None, |ui| {
                    ui.label(egui::RichText::new(&content).size(size).color(color));
                });
            }
            None => {
                let rect = ui.max_rect();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &content,
                    egui::FontId::proportional(size),
                    color,
                );
            }
        }
        false
    }
}
