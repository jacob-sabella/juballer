//! log_feed widget — rolling list of bus messages for a subscribed topic.
//!
//! Args:
//!   topic    : string (required) — bus topic to subscribe to
//!   max_rows : u64 (default 5)

use crate::theme::FONT_SMALL;
use crate::widget::card::draw_card;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::collections::VecDeque;
use std::time::Instant;
use tokio::sync::broadcast;

struct LogLine {
    ts: String,
    payload: String,
    arrived_at: Instant,
}

pub struct LogFeedWidget {
    topic: String,
    max_rows: usize,
    rx: Option<broadcast::Receiver<crate::bus::Event>>,
    lines: VecDeque<LogLine>,
    total_events: usize,
}

impl WidgetBuildFromArgs for LogFeedWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("log_feed requires args.topic (string)".into()))?
            .to_string();
        let max_rows = args
            .get("max_rows")
            .and_then(|v| v.as_integer())
            .map(|i| i.clamp(1, 50) as usize)
            .unwrap_or(5);
        Ok(Self {
            topic,
            max_rows,
            rx: None,
            lines: VecDeque::new(),
            total_events: 0,
        })
    }
}

impl Widget for LogFeedWidget {
    fn on_will_appear(&mut self, cx: &mut WidgetCx<'_>) {
        self.rx = Some(cx.bus.subscribe());
        self.total_events = 0;
    }

    fn on_will_disappear(&mut self, _cx: &mut WidgetCx<'_>) {
        self.rx = None;
        self.lines.clear();
        self.total_events = 0;
    }

    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        if let Some(rx) = self.rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        if ev.topic == self.topic {
                            let payload = compact(&ev.data);
                            if self.lines.len() >= self.max_rows {
                                self.lines.pop_front();
                            }
                            self.lines.push_back(LogLine {
                                ts: short_ts(),
                                payload,
                                arrived_at: Instant::now(),
                            });
                            self.total_events += 1;
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

        let count_text = format!("{} events", self.total_events);
        let badge = Some((count_text.as_str(), theme.subtext0));
        let lines = &self.lines;
        draw_card(ui, &theme, Some("log"), badge, |ui| {
            if lines.is_empty() {
                let rect = ui.max_rect();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "no recent action",
                    egui::FontId::proportional(FONT_SMALL),
                    theme.overlay1,
                );
                return;
            }
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, line) in lines.iter().enumerate() {
                let row_h = 14.0;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Sense::hover(),
                );
                if i % 2 == 1 {
                    let s = theme.surface1;
                    ui.painter().rect_filled(
                        rect,
                        egui::Rounding::same(3.0),
                        egui::Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 100),
                    );
                }
                let age_ms = line.arrived_at.elapsed().as_millis() as f32;
                let fade = (age_ms / 250.0).clamp(0.0, 1.0);
                let txt_color = egui::Color32::from_rgba_unmultiplied(
                    theme.text.r(),
                    theme.text.g(),
                    theme.text.b(),
                    (fade * 255.0) as u8,
                );
                let ts_color = egui::Color32::from_rgba_unmultiplied(
                    theme.subtext0.r(),
                    theme.subtext0.g(),
                    theme.subtext0.b(),
                    (fade * 255.0) as u8,
                );
                let painter = ui.painter();
                painter.text(
                    rect.left_center() + egui::vec2(4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &line.ts,
                    egui::FontId::monospace(FONT_SMALL),
                    ts_color,
                );
                let ts_w = (line.ts.len() as f32) * FONT_SMALL * 0.6 + 12.0;
                painter.text(
                    rect.left_center() + egui::vec2(4.0 + ts_w, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &line.payload,
                    egui::FontId::monospace(FONT_SMALL),
                    txt_color,
                );
            }
        });
        !self.lines.is_empty()
    }
}

fn short_ts() -> String {
    let now = chrono::Local::now();
    now.format("%H:%M:%S").to_string()
}

fn compact(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => v.to_string(),
        _ => v.to_string().trim_matches('"').to_string(),
    }
}
