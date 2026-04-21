//! notification_toast widget — fades in/out a recent bus event, anchored top-right of its pane.
//!
//! Args:
//!   prefixes        : array of string (default ["action."]) — match a topic if any prefix is a prefix of it
//!   dismiss_after_ms: u64 (default 3000) — clear if no new event in this many ms

use crate::theme::{FONT_BODY, FONT_SMALL};
use crate::widget::card::CARD_RADIUS;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::Result;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

#[derive(Clone)]
struct Toast {
    topic: String,
    summary: String,
    accent: ToastLevel,
    fired_at: Instant,
}

#[derive(Clone, Copy)]
enum ToastLevel {
    Ok,
    Warn,
    Err,
    Info,
}

pub struct NotificationToastWidget {
    prefixes: Vec<String>,
    dismiss_after: Duration,
    rx: Option<broadcast::Receiver<crate::bus::Event>>,
    current: Option<Toast>,
}

impl WidgetBuildFromArgs for NotificationToastWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let prefixes = args
            .get("prefixes")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec!["action.".to_string()]);
        let dismiss_after_ms = args
            .get("dismiss_after_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(200) as u64)
            .unwrap_or(3000);
        Ok(Self {
            prefixes,
            dismiss_after: Duration::from_millis(dismiss_after_ms),
            rx: None,
            current: None,
        })
    }
}

fn classify(data: &serde_json::Value) -> ToastLevel {
    if let Some(s) = data.get("status").and_then(|v| v.as_u64()) {
        if (200..300).contains(&s) {
            ToastLevel::Ok
        } else if (300..400).contains(&s) {
            ToastLevel::Warn
        } else {
            ToastLevel::Err
        }
    } else if data.get("error").is_some() {
        ToastLevel::Err
    } else if data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        ToastLevel::Ok
    } else {
        ToastLevel::Info
    }
}

fn summarize(data: &serde_json::Value) -> String {
    if let Some(s) = data.get("status").and_then(|v| v.as_u64()) {
        return format!("{}", s);
    }
    if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
        return truncate(err, 48);
    }
    if data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return "ok".to_string();
    }
    truncate(&data.to_string(), 48)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out = s.chars().take(max - 1).collect::<String>();
        out.push('…');
        out
    }
}

fn short_topic(topic: &str) -> String {
    let no_prefix = topic.strip_prefix("action.").unwrap_or(topic);
    no_prefix.split(':').next().unwrap_or(no_prefix).to_string()
}

impl Widget for NotificationToastWidget {
    fn on_will_appear(&mut self, cx: &mut WidgetCx<'_>) {
        self.rx = Some(cx.bus.subscribe());
    }

    fn on_will_disappear(&mut self, _cx: &mut WidgetCx<'_>) {
        self.rx = None;
        self.current = None;
    }

    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        if let Some(rx) = self.rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        if self
                            .prefixes
                            .iter()
                            .any(|p| ev.topic.starts_with(p.as_str()))
                        {
                            self.current = Some(Toast {
                                topic: short_topic(&ev.topic),
                                summary: summarize(&ev.data),
                                accent: classify(&ev.data),
                                fired_at: Instant::now(),
                            });
                        }
                    }
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        self.rx = None;
                        break;
                    }
                }
            }
        }

        if let Some(t) = &self.current {
            if t.fired_at.elapsed() >= self.dismiss_after {
                self.current = None;
            }
        }

        let Some(t) = self.current.clone() else {
            return false;
        };
        let age_ms = t.fired_at.elapsed().as_millis() as f32;
        let total_ms = self.dismiss_after.as_millis() as f32;
        let appear = (age_ms / 200.0).clamp(0.0, 1.0);
        let dismiss = (1.0 - (age_ms / total_ms)).clamp(0.0, 1.0);
        let alpha_f = appear.min(dismiss);
        let alpha = (alpha_f * 255.0) as u8;

        let accent = match t.accent {
            ToastLevel::Ok => theme.ok,
            ToastLevel::Warn => theme.warn,
            ToastLevel::Err => theme.err,
            ToastLevel::Info => theme.accent_alt,
        };

        let pane = ui.max_rect();
        let toast_w = (pane.width() * 0.6).clamp(140.0, 260.0);
        let toast_h = pane.height().min(48.0);
        let margin = 6.0;
        let toast_rect = egui::Rect::from_min_size(
            egui::pos2(pane.max.x - toast_w - margin, pane.min.y + margin),
            egui::vec2(toast_w, toast_h),
        );

        let painter = ui.painter();
        let bg = theme.surface0;
        let bg_color = egui::Color32::from_rgba_unmultiplied(
            bg.r(),
            bg.g(),
            bg.b(),
            ((alpha as f32) * 0.9) as u8,
        );
        let border_color = egui::Color32::from_rgba_unmultiplied(
            theme.surface1.r(),
            theme.surface1.g(),
            theme.surface1.b(),
            alpha,
        );
        painter.rect_filled(
            toast_rect,
            egui::CornerRadius::same((CARD_RADIUS) as u8),
            bg_color,
        );
        painter.rect_stroke(
            toast_rect,
            egui::CornerRadius::same((CARD_RADIUS) as u8),
            egui::Stroke::new(1.0, border_color),
            egui::StrokeKind::Middle,
        );

        // Accent left-edge bar (4px wide), fully opaque color.
        let bar_rect = egui::Rect::from_min_max(
            toast_rect.min + egui::vec2(1.0, 5.0),
            egui::pos2(toast_rect.min.x + 5.0, toast_rect.max.y - 5.0),
        );
        painter.rect_filled(
            bar_rect,
            egui::CornerRadius::same(2),
            egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
        );

        let text_x = toast_rect.min.x + 12.0;
        let topic_color = egui::Color32::from_rgba_unmultiplied(
            theme.subtext0.r(),
            theme.subtext0.g(),
            theme.subtext0.b(),
            alpha,
        );
        let summary_color = egui::Color32::from_rgba_unmultiplied(
            theme.text.r(),
            theme.text.g(),
            theme.text.b(),
            alpha,
        );
        painter.text(
            egui::pos2(text_x, toast_rect.min.y + 8.0),
            egui::Align2::LEFT_TOP,
            t.topic.to_uppercase(),
            egui::FontId::proportional(FONT_SMALL),
            topic_color,
        );
        painter.text(
            egui::pos2(text_x, toast_rect.min.y + 22.0),
            egui::Align2::LEFT_TOP,
            &t.summary,
            egui::FontId::proportional(FONT_BODY),
            summary_color,
        );

        true
    }
}
