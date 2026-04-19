//! http_probe widget — periodic GET → colored status card.
//!
//! Args:
//!   url         : string (required)
//!   label       : string (optional) — card header
//!   interval_ms : u64 (default 5000)

use crate::theme::{ease_to, FONT_BODY, FONT_HEADER, FONT_SMALL};
use crate::widget::card::draw_card;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SPARK_CAPACITY: usize = 16;

#[derive(Clone)]
struct ProbeState {
    status: Option<u16>,
    last_error: Option<String>,
    latency_ms: Option<f32>,
    last_fetched_at: Option<Instant>,
    history: VecDeque<f32>,
}

pub struct HttpProbeWidget {
    url: String,
    label: String,
    interval: Duration,
    state: Arc<Mutex<ProbeState>>,
    probe_in_flight: bool,
    last_fired: Option<Instant>,
    last_frame: Option<Instant>,
    latency_display: f32,
}

impl WidgetBuildFromArgs for HttpProbeWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("http_probe requires args.url (string)".into()))?
            .to_string();
        let label = args
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("probe")
            .to_string();
        let interval_ms = args
            .get("interval_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(500) as u64)
            .unwrap_or(5000);
        Ok(Self {
            url,
            label,
            interval: Duration::from_millis(interval_ms),
            state: Arc::new(Mutex::new(ProbeState {
                status: None,
                last_error: None,
                latency_ms: None,
                last_fetched_at: None,
                history: VecDeque::with_capacity(SPARK_CAPACITY),
            })),
            probe_in_flight: false,
            last_fired: None,
            last_frame: None,
            latency_display: 0.0,
        })
    }
}

impl Widget for HttpProbeWidget {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let now = Instant::now();
        let should_fire = !self.probe_in_flight
            && self
                .last_fired
                .map(|t| now.duration_since(t) >= self.interval)
                .unwrap_or(true);

        if should_fire {
            self.last_fired = Some(now);
            self.probe_in_flight = true;
            let url = self.url.clone();
            let state = self.state.clone();
            cx.rt.spawn(async move {
                let started = Instant::now();
                let result: std::result::Result<u16, String> = match reqwest::Client::builder()
                    .timeout(Duration::from_secs(3))
                    .build()
                {
                    Ok(client) => match client.get(&url).send().await {
                        Ok(r) => Ok(r.status().as_u16()),
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(e.to_string()),
                };
                let latency = started.elapsed().as_secs_f32() * 1000.0;
                let mut st = state.lock().unwrap();
                match result {
                    Ok(code) => {
                        st.status = Some(code);
                        st.last_error = None;
                        st.latency_ms = Some(latency);
                        if st.history.len() >= SPARK_CAPACITY {
                            st.history.pop_front();
                        }
                        st.history.push_back(latency);
                    }
                    Err(e) => {
                        st.status = None;
                        st.last_error = Some(e);
                        st.latency_ms = None;
                    }
                }
                st.last_fetched_at = Some(Instant::now());
            });
        }

        let snapshot = self.state.lock().unwrap().clone();
        if snapshot.last_fetched_at.is_some() {
            self.probe_in_flight = false;
        }

        let dt = self
            .last_frame
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(1.0 / 60.0);
        self.last_frame = Some(now);
        if let Some(l) = snapshot.latency_ms {
            ease_to(&mut self.latency_display, l, dt, 0.25);
        }

        let (status_color, status_text) = match snapshot.status {
            Some(c) if (200..300).contains(&c) => (theme.ok, format!("{c}")),
            Some(c) if (300..400).contains(&c) => (theme.warn, format!("{c}")),
            Some(c) => (theme.err, format!("{c}")),
            None if snapshot.last_error.is_some() => (theme.err, "FAIL".into()),
            None => (theme.overlay0, "…".into()),
        };

        let label = self.label.clone();
        let latency_display = self.latency_display;
        let history = snapshot.history.clone();
        let last_error = snapshot.last_error.clone();
        let has_latency = snapshot.latency_ms.is_some();

        draw_card(ui, &theme, Some(&label), None, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&status_text)
                        .size(FONT_HEADER)
                        .strong()
                        .color(status_color),
                );
                if has_latency {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{:.0} ms", latency_display))
                                .size(FONT_BODY)
                                .color(theme.subtext1),
                        );
                    });
                }
            });
            if history.len() >= 2 {
                paint_sparkline(ui, &history, theme.accent_alt, theme.surface1);
            }
            if let Some(e) = last_error {
                let msg = if e.len() > 48 {
                    format!("{}…", &e[..48])
                } else {
                    e
                };
                ui.label(egui::RichText::new(msg).size(FONT_SMALL).color(theme.err));
            }
        });

        true
    }
}

fn paint_sparkline(
    ui: &mut egui::Ui,
    samples: &VecDeque<f32>,
    stroke_color: egui::Color32,
    track_color: egui::Color32,
) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::same(2.0), track_color);
    if samples.len() < 2 {
        return;
    }
    let min = samples.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = samples.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = (max - min).max(1.0);
    let n = samples.len();
    let step = rect.width() / (n as f32 - 1.0);
    let mut prev: Option<egui::Pos2> = None;
    for (i, s) in samples.iter().enumerate() {
        let x = rect.min.x + step * i as f32;
        let y = rect.max.y - ((s - min) / range) * (rect.height() - 2.0) - 1.0;
        let p = egui::pos2(x, y);
        if let Some(prev_p) = prev {
            painter.line_segment([prev_p, p], egui::Stroke::new(1.5, stroke_color));
        }
        prev = Some(p);
    }
}
