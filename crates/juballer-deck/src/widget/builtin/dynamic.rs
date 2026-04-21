use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use juballer_deck_protocol::view::{Align, IconSrc, ImageFit, ImageSrc, ViewNode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
struct ImageSlot {
    bytes: Option<Vec<u8>>,
    err: Option<String>,
    in_flight: bool,
}

type ImageCache = Arc<Mutex<HashMap<String, ImageSlot>>>;

pub struct DynamicWidget {
    tree_key: String,
    placeholder: String,
    images: ImageCache,
}

impl WidgetBuildFromArgs for DynamicWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let tree_key = args
            .get("tree_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("dynamic widget requires args.tree_key (string)".into()))?
            .to_string();
        let placeholder = args
            .get("placeholder")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(Self {
            tree_key,
            placeholder,
            images: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

impl Widget for DynamicWidget {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let snapshot = cx
            .view_trees
            .read()
            .ok()
            .and_then(|m| m.get(&self.tree_key).cloned());
        let pane_rect = ui.max_rect();
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(pane_rect));
        inner.set_clip_rect(pane_rect);
        match snapshot {
            Some(tree) => {
                let rcx = RenderCx {
                    bus: cx.bus,
                    rt: cx.rt,
                    images: self.images.clone(),
                };
                render_node(&mut inner, &tree, &rcx);
            }
            None => {
                if !self.placeholder.is_empty() {
                    inner.label(
                        egui::RichText::new(&self.placeholder).color(egui::Color32::DARK_GRAY),
                    );
                }
            }
        }
        false
    }
}

struct RenderCx<'a> {
    bus: &'a crate::bus::EventBus,
    rt: &'a tokio::runtime::Handle,
    images: ImageCache,
}

fn render_node(ui: &mut egui::Ui, node: &ViewNode, cx: &RenderCx<'_>) {
    match node {
        ViewNode::Vstack {
            gap,
            align,
            children,
        } => {
            let layout = egui::Layout::top_down(align_to_egui(*align));
            ui.with_layout(layout, |ui| {
                ui.spacing_mut().item_spacing.y = *gap;
                for child in children {
                    render_node(ui, child, cx);
                }
            });
        }
        ViewNode::Hstack {
            gap,
            align,
            children,
        } => {
            let layout = egui::Layout::left_to_right(align_to_egui(*align));
            ui.with_layout(layout, |ui| {
                ui.spacing_mut().item_spacing.x = *gap;
                for child in children {
                    render_node(ui, child, cx);
                }
            });
        }
        ViewNode::Text {
            value,
            size,
            color,
            weight,
        } => {
            let mut rt = egui::RichText::new(value);
            if let Some(s) = size {
                rt = rt.size(*s);
            }
            if let Some(c) = color.as_deref().and_then(parse_color) {
                rt = rt.color(c);
            }
            if matches!(weight.as_deref(), Some("bold")) {
                rt = rt.strong();
            }
            ui.label(rt);
        }
        ViewNode::Icon { src, size } => match src {
            IconSrc::Emoji { emoji } => {
                let mut rt = egui::RichText::new(emoji);
                if let Some(s) = size {
                    rt = rt.size(*s);
                }
                ui.label(rt);
            }
            IconSrc::Path { path } => {
                ui.label(
                    egui::RichText::new(format!("[{}]", path))
                        .small()
                        .color(egui::Color32::DARK_GRAY),
                );
            }
        },
        ViewNode::Bar {
            value,
            color,
            label,
        } => {
            let mut bar = egui::ProgressBar::new(value.clamp(0.0, 1.0));
            if let Some(l) = label {
                bar = bar.text(l.clone());
            }
            if let Some(c) = color.as_deref().and_then(parse_color) {
                bar = bar.fill(c);
            }
            ui.add(bar);
        }
        ViewNode::Spacer { size } => {
            ui.add_space(*size);
        }
        ViewNode::Divider => {
            ui.separator();
        }
        ViewNode::Image {
            src,
            width,
            height,
            fit,
        } => render_image(ui, src, *width, *height, fit.unwrap_or_default(), cx),
        ViewNode::Button {
            label,
            action,
            args,
            color,
        } => {
            let text = match color.as_deref().and_then(parse_color) {
                Some(c) => egui::RichText::new(label).color(c),
                None => egui::RichText::new(label),
            };
            if ui.button(text).clicked() {
                cx.bus.publish(
                    "widget.action_request",
                    serde_json::json!({
                        "action": action,
                        "args": args.clone().unwrap_or(serde_json::json!({})),
                        "cell": [0, 0],
                    }),
                );
            }
        }
        ViewNode::Plot {
            values,
            color,
            height,
            label,
        } => render_plot(ui, values, color.as_deref(), *height, label.as_deref()),
        ViewNode::Table {
            headers,
            rows,
            header_color,
        } => render_table(ui, headers, rows, header_color.as_deref()),
        ViewNode::Scroll { child, height } => {
            let mut area = egui::ScrollArea::vertical();
            if let Some(h) = height {
                area = area.max_height(*h);
            }
            area.show(ui, |ui| render_node(ui, child, cx));
        }
        ViewNode::Padding {
            child,
            all,
            top,
            right,
            bottom,
            left,
        } => {
            let a = all.unwrap_or(0.0);
            let t = top.unwrap_or(a);
            let r = right.unwrap_or(a);
            let b = bottom.unwrap_or(a);
            let l = left.unwrap_or(a);
            ui.vertical(|ui| {
                ui.add_space(t);
                ui.horizontal(|ui| {
                    ui.add_space(l);
                    ui.vertical(|ui| render_node(ui, child, cx));
                    ui.add_space(r);
                });
                ui.add_space(b);
            });
        }
        ViewNode::Bg {
            child,
            color,
            rounding,
        } => {
            let fill = parse_color(color).unwrap_or(egui::Color32::TRANSPARENT);
            let rad = rounding.unwrap_or(0.0);
            egui::Frame::NONE
                .fill(fill)
                .corner_radius(egui::CornerRadius::same((rad) as u8))
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| render_node(ui, child, cx));
        }
        ViewNode::Progress {
            value,
            max,
            color,
            label,
            show_percent,
        } => {
            let m = max.unwrap_or(1.0).max(f32::EPSILON);
            let frac = (value / m).clamp(0.0, 1.0);
            let mut bar = egui::ProgressBar::new(frac);
            let show_pct = show_percent.unwrap_or(true);
            let txt = match (label.as_deref(), show_pct) {
                (Some(l), true) => format!("{} ({:.0}%)", l, frac * 100.0),
                (Some(l), false) => l.to_string(),
                (None, true) => format!("{:.0}%", frac * 100.0),
                (None, false) => String::new(),
            };
            if !txt.is_empty() {
                bar = bar.text(txt);
            }
            if let Some(c) = color.as_deref().and_then(parse_color) {
                bar = bar.fill(c);
            }
            ui.add(bar);
        }
        ViewNode::Kpi {
            value,
            label,
            delta,
            delta_positive,
            color,
        } => {
            ui.vertical(|ui| {
                if let Some(l) = label {
                    ui.label(
                        egui::RichText::new(l)
                            .size(11.0)
                            .color(egui::Color32::from_rgb(0xa6, 0xad, 0xc8)),
                    );
                }
                let mut v = egui::RichText::new(value).size(28.0).strong();
                if let Some(c) = color.as_deref().and_then(parse_color) {
                    v = v.color(c);
                }
                ui.label(v);
                if let Some(d) = delta {
                    let dc = match delta_positive {
                        Some(true) => egui::Color32::from_rgb(0xa6, 0xe3, 0xa1),
                        Some(false) => egui::Color32::from_rgb(0xf3, 0x8b, 0xa8),
                        None => egui::Color32::from_rgb(0xa6, 0xad, 0xc8),
                    };
                    ui.label(egui::RichText::new(d).size(12.0).color(dc));
                }
            });
        }
    }
}

fn render_image(
    ui: &mut egui::Ui,
    src: &ImageSrc,
    width: Option<f32>,
    height: Option<f32>,
    fit: ImageFit,
    cx: &RenderCx<'_>,
) {
    let (cache_key, bytes) = resolve_image_bytes(src, cx);
    let bytes = match bytes {
        Some(b) => b,
        None => {
            ui.label(
                egui::RichText::new("…")
                    .small()
                    .color(egui::Color32::DARK_GRAY),
            );
            return;
        }
    };
    let img = match image::load_from_memory(&bytes) {
        Ok(i) => i,
        Err(e) => {
            ui.label(
                egui::RichText::new(format!("!decode: {}", e))
                    .small()
                    .color(egui::Color32::from_rgb(0xf3, 0x8b, 0xa8)),
            );
            return;
        }
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let tex = ui
        .ctx()
        .load_texture(cache_key, color_img, egui::TextureOptions::LINEAR);
    let avail = ui.available_size();
    let w = width.unwrap_or(avail.x.max(1.0));
    let h = height.unwrap_or(avail.y.max(1.0));
    let mut image_w = egui::Image::new(egui::load::SizedTexture::from_handle(&tex));
    image_w = match fit {
        ImageFit::Contain => image_w.fit_to_exact_size(egui::vec2(w, h)),
        ImageFit::Cover => image_w
            .fit_to_exact_size(egui::vec2(w, h))
            .maintain_aspect_ratio(false),
        ImageFit::Fill => image_w
            .fit_to_exact_size(egui::vec2(w, h))
            .maintain_aspect_ratio(false),
    };
    ui.add(image_w);
}

fn resolve_image_bytes(src: &ImageSrc, cx: &RenderCx<'_>) -> (String, Option<Vec<u8>>) {
    match src {
        ImageSrc::Path { path } => {
            let key = format!("dyn_img://path/{}", path);
            let mut map = cx.images.lock().unwrap();
            let slot = map.entry(key.clone()).or_default();
            if slot.bytes.is_none() && slot.err.is_none() {
                match std::fs::read(path) {
                    Ok(b) => slot.bytes = Some(b),
                    Err(e) => slot.err = Some(e.to_string()),
                }
            }
            (key, slot.bytes.clone())
        }
        ImageSrc::DataUrl { data_url } => {
            let key = format!("dyn_img://data/{:x}", simple_hash(data_url.as_bytes()));
            let mut map = cx.images.lock().unwrap();
            let slot = map.entry(key.clone()).or_default();
            if slot.bytes.is_none() && slot.err.is_none() {
                if let Some(idx) = data_url.find(',') {
                    let payload = &data_url[idx + 1..];
                    match B64.decode(payload.as_bytes()) {
                        Ok(b) => slot.bytes = Some(b),
                        Err(e) => slot.err = Some(e.to_string()),
                    }
                } else {
                    slot.err = Some("malformed data_url".into());
                }
            }
            (key, slot.bytes.clone())
        }
        ImageSrc::Url { url } => {
            let key = format!("dyn_img://url/{}", url);
            let mut map = cx.images.lock().unwrap();
            let slot = map.entry(key.clone()).or_default();
            let cached = slot.bytes.clone();
            if cached.is_none() && !slot.in_flight && slot.err.is_none() {
                slot.in_flight = true;
                let url_c = url.clone();
                let images = cx.images.clone();
                let key_c = key.clone();
                cx.rt.spawn(async move {
                    let result = async {
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(10))
                            .build()?;
                        let r = client.get(&url_c).send().await?;
                        let b = r.error_for_status()?.bytes().await?;
                        Ok::<Vec<u8>, reqwest::Error>(b.to_vec())
                    }
                    .await;
                    let mut map = images.lock().unwrap();
                    let slot = map.entry(key_c).or_default();
                    slot.in_flight = false;
                    match result {
                        Ok(b) => {
                            slot.bytes = Some(b);
                            slot.err = None;
                        }
                        Err(e) => slot.err = Some(e.to_string()),
                    }
                });
            }
            (key, cached)
        }
    }
}

fn render_plot(
    ui: &mut egui::Ui,
    values: &[f32],
    color: Option<&str>,
    height: Option<f32>,
    label: Option<&str>,
) {
    let h = height.unwrap_or(40.0);
    let avail_w = ui.available_width().max(10.0);
    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(avail_w, h), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(2),
        egui::Color32::from_rgba_unmultiplied(0x1e, 0x1e, 0x2e, 128),
    );
    if values.len() >= 2 {
        let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let range = (max - min).max(f32::EPSILON);
        let step = rect.width() / (values.len() - 1) as f32;
        let stroke = egui::Stroke::new(
            1.5,
            color
                .and_then(parse_color)
                .unwrap_or(egui::Color32::from_rgb(0x89, 0xb4, 0xfa)),
        );
        for i in 0..values.len() - 1 {
            let x0 = rect.left() + step * i as f32;
            let x1 = rect.left() + step * (i + 1) as f32;
            let y0 = rect.bottom() - ((values[i] - min) / range) * rect.height();
            let y1 = rect.bottom() - ((values[i + 1] - min) / range) * rect.height();
            painter.line_segment([egui::pos2(x0, y0), egui::pos2(x1, y1)], stroke);
        }
    }
    if let Some(l) = label {
        painter.text(
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            l,
            egui::FontId::proportional(10.0),
            egui::Color32::from_rgb(0xa6, 0xad, 0xc8),
        );
    }
}

fn render_table(
    ui: &mut egui::Ui,
    headers: &[String],
    rows: &[Vec<String>],
    header_color: Option<&str>,
) {
    egui::Grid::new(egui::Id::new(("dyn_table", headers.len(), rows.len())))
        .striped(true)
        .show(ui, |ui| {
            let hc = header_color
                .and_then(parse_color)
                .unwrap_or(egui::Color32::from_rgb(0x89, 0xb4, 0xfa));
            for h in headers {
                ui.label(egui::RichText::new(h).strong().color(hc));
            }
            ui.end_row();
            for row in rows {
                for cell in row {
                    ui.label(cell);
                }
                for _ in row.len()..headers.len() {
                    ui.label("");
                }
                ui.end_row();
            }
        });
}

fn simple_hash(b: &[u8]) -> u64 {
    let mut h: u64 = 1469598103934665603;
    for byte in b {
        h ^= *byte as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn align_to_egui(a: Align) -> egui::Align {
    match a {
        Align::Start => egui::Align::Min,
        Align::Center => egui::Align::Center,
        Align::End => egui::Align::Max,
    }
}

pub(crate) fn parse_color(s: &str) -> Option<egui::Color32> {
    if let Some(c) = named_color(s) {
        return Some(c);
    }
    let s = s.strip_prefix('#')?;
    let bytes = match s.len() {
        6 => hex::decode(s).ok()?,
        8 => hex::decode(s).ok()?,
        _ => return None,
    };
    let a = if bytes.len() == 4 { bytes[3] } else { 0xff };
    Some(egui::Color32::from_rgba_unmultiplied(
        bytes[0], bytes[1], bytes[2], a,
    ))
}

fn named_color(s: &str) -> Option<egui::Color32> {
    match s {
        "rosewater" => Some(egui::Color32::from_rgb(0xf5, 0xe0, 0xdc)),
        "flamingo" => Some(egui::Color32::from_rgb(0xf2, 0xcd, 0xcd)),
        "pink" => Some(egui::Color32::from_rgb(0xf5, 0xc2, 0xe7)),
        "mauve" => Some(egui::Color32::from_rgb(0xcb, 0xa6, 0xf7)),
        "red" => Some(egui::Color32::from_rgb(0xf3, 0x8b, 0xa8)),
        "maroon" => Some(egui::Color32::from_rgb(0xeb, 0xa0, 0xac)),
        "peach" => Some(egui::Color32::from_rgb(0xfa, 0xb3, 0x87)),
        "yellow" => Some(egui::Color32::from_rgb(0xf9, 0xe2, 0xaf)),
        "green" => Some(egui::Color32::from_rgb(0xa6, 0xe3, 0xa1)),
        "teal" => Some(egui::Color32::from_rgb(0x94, 0xe2, 0xd5)),
        "sky" => Some(egui::Color32::from_rgb(0x89, 0xdc, 0xeb)),
        "sapphire" => Some(egui::Color32::from_rgb(0x74, 0xc7, 0xec)),
        "blue" => Some(egui::Color32::from_rgb(0x89, 0xb4, 0xfa)),
        "lavender" => Some(egui::Color32::from_rgb(0xb4, 0xbe, 0xfe)),
        "text" => Some(egui::Color32::from_rgb(0xcd, 0xd6, 0xf4)),
        "subtext1" => Some(egui::Color32::from_rgb(0xba, 0xc2, 0xde)),
        "subtext0" => Some(egui::Color32::from_rgb(0xa6, 0xad, 0xc8)),
        "overlay2" => Some(egui::Color32::from_rgb(0x9c, 0xa0, 0xb0)),
        "overlay1" => Some(egui::Color32::from_rgb(0x7f, 0x84, 0x9c)),
        "overlay0" => Some(egui::Color32::from_rgb(0x6c, 0x70, 0x86)),
        "surface2" => Some(egui::Color32::from_rgb(0x58, 0x5b, 0x70)),
        "surface1" => Some(egui::Color32::from_rgb(0x45, 0x47, 0x5a)),
        "surface0" => Some(egui::Color32::from_rgb(0x31, 0x32, 0x44)),
        "base" => Some(egui::Color32::from_rgb(0x1e, 0x1e, 0x2e)),
        "mantle" => Some(egui::Color32::from_rgb(0x18, 0x18, 0x25)),
        "crust" => Some(egui::Color32::from_rgb(0x11, 0x11, 0x1b)),
        "white" => Some(egui::Color32::WHITE),
        "black" => Some(egui::Color32::BLACK),
        "transparent" => Some(egui::Color32::TRANSPARENT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use juballer_deck_protocol::view::{ImageFit, ImageSrc};

    #[test]
    fn parse_color_rgb() {
        let c = parse_color("#cdd6f4").unwrap();
        assert_eq!(c.r(), 0xcd);
        assert_eq!(c.g(), 0xd6);
        assert_eq!(c.b(), 0xf4);
        assert_eq!(c.a(), 0xff);
    }

    #[test]
    fn parse_color_rgba() {
        let c = parse_color("#ffffff80").unwrap();
        assert_eq!(c.a(), 0x80);
    }

    #[test]
    fn parse_color_invalid() {
        assert!(parse_color("nope").is_none());
        assert!(parse_color("#xyz").is_none());
        assert!(parse_color("#abcd").is_none());
        assert!(parse_color("123456").is_none());
    }

    #[test]
    fn parse_color_named() {
        assert_eq!(parse_color("green").unwrap().r(), 0xa6);
        assert_eq!(parse_color("mauve").unwrap().r(), 0xcb);
        assert_eq!(parse_color("blue").unwrap().b(), 0xfa);
        assert_eq!(parse_color("red").unwrap().g(), 0x8b);
        assert!(parse_color("bogus").is_none());
    }

    #[test]
    fn dynamic_widget_requires_tree_key() {
        let args = toml::Table::new();
        assert!(DynamicWidget::from_args(&args).is_err());
    }

    #[test]
    fn dynamic_widget_builds_with_args() {
        let mut args = toml::Table::new();
        args.insert("tree_key".into(), toml::Value::String("disc".into()));
        args.insert("placeholder".into(), toml::Value::String("nope".into()));
        let w = DynamicWidget::from_args(&args).unwrap();
        assert_eq!(w.tree_key, "disc");
        assert_eq!(w.placeholder, "nope");
    }

    fn png_1x1() -> Vec<u8> {
        // Smallest valid 1x1 red PNG.
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x5b, 0xe2, 0x15,
            0x5e, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]
    }

    #[test]
    fn render_node_smoke_every_variant() {
        let ctx = egui::Context::default();
        let raw_input = egui::RawInput::default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let images: ImageCache = Arc::new(Mutex::new(HashMap::new()));
        let bus = crate::bus::EventBus::default();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), png_1x1()).unwrap();

        let data_url = format!("data:image/png;base64,{}", B64.encode(png_1x1()));

        #[allow(deprecated)]
        let _ = ctx.run(raw_input, |ctx| {
            #[allow(deprecated)]
            egui::CentralPanel::default().show(ctx, |ui| {
                let rcx = RenderCx {
                    bus: &bus,
                    rt: rt.handle(),
                    images: images.clone(),
                };
                let tree = ViewNode::Vstack {
                    gap: 4.0,
                    align: Align::Start,
                    children: vec![
                        ViewNode::Text {
                            value: "hello".into(),
                            size: Some(18.0),
                            color: Some("green".into()),
                            weight: Some("bold".into()),
                        },
                        ViewNode::Icon {
                            src: IconSrc::Emoji {
                                emoji: "🎤".into()
                            },
                            size: Some(24.0),
                        },
                        ViewNode::Icon {
                            src: IconSrc::Path {
                                path: "/x.png".into(),
                            },
                            size: None,
                        },
                        ViewNode::Bar {
                            value: 0.5,
                            color: Some("#a6e3a1".into()),
                            label: Some("lbl".into()),
                        },
                        ViewNode::Image {
                            src: ImageSrc::Path {
                                path: tmp.path().to_string_lossy().to_string(),
                            },
                            width: Some(16.0),
                            height: Some(16.0),
                            fit: Some(ImageFit::Contain),
                        },
                        ViewNode::Image {
                            src: ImageSrc::DataUrl {
                                data_url: data_url.clone(),
                            },
                            width: Some(16.0),
                            height: Some(16.0),
                            fit: None,
                        },
                        ViewNode::Button {
                            label: "Go".into(),
                            action: "deck.page_goto".into(),
                            args: Some(serde_json::json!({"page":"home"})),
                            color: Some("mauve".into()),
                        },
                        ViewNode::Plot {
                            values: vec![0.0, 1.0, 0.5, 0.8, 0.2],
                            color: Some("blue".into()),
                            height: Some(20.0),
                            label: Some("plot".into()),
                        },
                        ViewNode::Table {
                            headers: vec!["a".into(), "b".into()],
                            rows: vec![vec!["1".into(), "2".into()]],
                            header_color: Some("mauve".into()),
                        },
                        ViewNode::Scroll {
                            child: Box::new(ViewNode::Text {
                                value: "x".into(),
                                size: None,
                                color: None,
                                weight: None,
                            }),
                            height: Some(80.0),
                        },
                        ViewNode::Padding {
                            child: Box::new(ViewNode::Divider),
                            all: Some(4.0),
                            top: None,
                            right: None,
                            bottom: None,
                            left: None,
                        },
                        ViewNode::Bg {
                            child: Box::new(ViewNode::Text {
                                value: "bg".into(),
                                size: None,
                                color: None,
                                weight: None,
                            }),
                            color: "surface0".into(),
                            rounding: Some(4.0),
                        },
                        ViewNode::Progress {
                            value: 42.0,
                            max: Some(100.0),
                            color: Some("green".into()),
                            label: Some("CPU".into()),
                            show_percent: Some(true),
                        },
                        ViewNode::Kpi {
                            value: "123".into(),
                            label: Some("users".into()),
                            delta: Some("+5".into()),
                            delta_positive: Some(true),
                            color: Some("lavender".into()),
                        },
                        ViewNode::Hstack {
                            gap: 6.0,
                            align: Align::End,
                            children: vec![ViewNode::Divider],
                        },
                        ViewNode::Spacer { size: 8.0 },
                        ViewNode::Divider,
                    ],
                };
                render_node(ui, &tree, &rcx);
            });
        });
    }
}
