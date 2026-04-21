//! image widget — displays a static image from disk, optionally wrapped in a card.
//!
//! Args:
//!   path  : string (required) — absolute path or relative to profile assets
//!   title : string (optional) — when present, wraps the image in a card header

use crate::theme::FONT_SMALL;
use crate::widget::card::{draw_card, CARD_RADIUS};
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::path::PathBuf;

pub struct ImageWidget {
    path: PathBuf,
    title: Option<String>,
    loaded: Option<Vec<u8>>,
}

impl WidgetBuildFromArgs for ImageWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("image widget requires args.path".into()))?;
        let title = args.get("title").and_then(|v| v.as_str()).map(String::from);
        Ok(Self {
            path: PathBuf::from(path),
            title,
            loaded: None,
        })
    }
}

impl Widget for ImageWidget {
    fn on_will_appear(&mut self, _cx: &mut WidgetCx<'_>) {
        if self.loaded.is_none() {
            self.loaded = std::fs::read(&self.path).ok();
        }
    }

    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let path_display = self.path.display().to_string();
        let loaded = self.loaded.as_ref().cloned();
        let title = self.title.clone();
        let render_inner = |ui: &mut egui::Ui| match loaded.as_ref() {
            Some(bytes) => match image::load_from_memory(bytes) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let uri = format!("bytes://{}", path_display);
                    let tex = ui
                        .ctx()
                        .load_texture(uri, color_img, egui::TextureOptions::LINEAR);
                    let avail = ui.available_size();
                    let w = rgba.width() as f32;
                    let h = rgba.height() as f32;
                    let ratio = (avail.x / w).min(avail.y / h).clamp(0.0, 1.0);
                    let draw_size = egui::vec2(w * ratio, h * ratio);
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::from_handle(&tex))
                            .fit_to_exact_size(draw_size)
                            .corner_radius(egui::CornerRadius::same((CARD_RADIUS * 0.5) as u8)),
                    );
                }
                Err(e) => {
                    ui.label(
                        egui::RichText::new(format!("!decode: {e}"))
                            .size(FONT_SMALL)
                            .color(theme.err),
                    );
                }
            },
            None => {
                ui.label(
                    egui::RichText::new(format!("!missing: {}", path_display))
                        .size(FONT_SMALL)
                        .color(theme.err),
                );
            }
        };

        match title {
            Some(t) => draw_card(ui, &theme, Some(&t), None, render_inner),
            None => draw_card(ui, &theme, None, None, render_inner),
        }
        false
    }
}
