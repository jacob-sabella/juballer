//! counter widget — renders a named counter's value inside a card, live-updates on bus events.
//!
//! Args:
//!   name  : string (required) — same counter name as used by counter.* actions
//!   label : string (optional, default = name)

use crate::theme::FONT_LARGE;
use crate::widget::card::draw_card;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::time::Instant;
use tokio::sync::broadcast;

pub struct CounterWidget {
    name: String,
    label: String,
    value: i64,
    rx: Option<broadcast::Receiver<crate::bus::Event>>,
    topic: String,
    bumped_at: Option<Instant>,
    last_delta: i64,
}

impl WidgetBuildFromArgs for CounterWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("counter widget requires name".into()))?
            .to_string();
        let label = args
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or(&name)
            .to_string();
        let topic = format!("counter.{}", name);
        Ok(Self {
            name,
            label,
            value: 0,
            rx: None,
            topic,
            bumped_at: None,
            last_delta: 0,
        })
    }
}

impl Widget for CounterWidget {
    fn on_will_appear(&mut self, cx: &mut WidgetCx<'_>) {
        let key = format!("counter:{}", self.name);
        self.value = cx
            .state
            .binding(&key)
            .and_then(|v| v.get("n"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        self.rx = Some(cx.bus.subscribe());
    }

    fn on_will_disappear(&mut self, _cx: &mut WidgetCx<'_>) {
        self.rx = None;
    }

    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        if let Some(rx) = self.rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        if ev.topic == self.topic {
                            if let Some(n) = ev.data.get("n").and_then(|v| v.as_i64()) {
                                if n != self.value {
                                    self.last_delta = n - self.value;
                                    self.bumped_at = Some(Instant::now());
                                }
                                self.value = n;
                            }
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

        let tint = self.bumped_at.and_then(|t| {
            let ms = t.elapsed().as_millis() as f32;
            if ms > 1000.0 {
                None
            } else {
                Some(1.0 - ms / 1000.0)
            }
        });

        let value_color = match tint {
            Some(t) => {
                let a = theme.accent;
                let base = theme.text;
                let mix = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t).round() as u8;
                egui::Color32::from_rgb(
                    mix(base.r(), a.r()),
                    mix(base.g(), a.g()),
                    mix(base.b(), a.b()),
                )
            }
            None => theme.text,
        };

        let delta_text = match tint {
            Some(_) if self.last_delta != 0 => {
                if self.last_delta > 0 {
                    Some((format!("↑{}", self.last_delta), theme.ok))
                } else {
                    Some((format!("↓{}", self.last_delta.abs()), theme.err))
                }
            }
            _ => None,
        };

        let label = self.label.clone();
        let value_str = self.value.to_string();
        let badge = delta_text.as_ref().map(|(s, c)| (s.as_str(), *c));
        draw_card(ui, &theme, Some(&label), badge, |ui| {
            let rect = ui.max_rect();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &value_str,
                egui::FontId::proportional(FONT_LARGE),
                value_color,
            );
            // Reserve space so the card isn't collapsed on next render.
            let _ = ui.allocate_space(egui::vec2(
                rect.width(),
                rect.height().max(FONT_LARGE * 1.2),
            ));
        });
        tint.is_some()
    }
}
