//! homelab_status widget — HTTP probes rendered as a card with a row per probe.
//!
//! Args:
//!   interval_ms : u64 (default 5000)
//!   probes      : array of table, each with { label: string, url: string }

use crate::theme::FONT_SMALL;
use crate::widget::card::draw_card;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::{Error, Result};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
struct Probe {
    label: String,
    url: String,
}

#[derive(Clone, Default)]
struct Snapshot {
    status: Option<u16>,
    err: Option<String>,
}

pub struct HomelabStatusWidget {
    interval: Duration,
    probes: Vec<Probe>,
    snapshots: Vec<Arc<Mutex<Snapshot>>>,
    last_fired: Option<Instant>,
    in_flight: bool,
}

impl WidgetBuildFromArgs for HomelabStatusWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let interval_ms = args
            .get("interval_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(1000) as u64)
            .unwrap_or(5000);
        let raw = args
            .get("probes")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Config("homelab_status requires probes (array)".into()))?;
        let probes: Vec<Probe> = raw
            .iter()
            .filter_map(|v| {
                let t = v.as_table()?;
                let label = t.get("label")?.as_str()?.to_string();
                let url = t.get("url")?.as_str()?.to_string();
                Some(Probe { label, url })
            })
            .collect();
        if probes.is_empty() {
            return Err(Error::Config("homelab_status: no valid probes".into()));
        }
        let snapshots = probes
            .iter()
            .map(|_| Arc::new(Mutex::new(Snapshot::default())))
            .collect();
        Ok(Self {
            interval: Duration::from_millis(interval_ms),
            probes,
            snapshots,
            last_fired: None,
            in_flight: false,
        })
    }
}

impl Widget for HomelabStatusWidget {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let now = Instant::now();
        let should_fire = !self.in_flight
            && self
                .last_fired
                .map(|t| now.duration_since(t) >= self.interval)
                .unwrap_or(true);

        if should_fire {
            self.last_fired = Some(now);
            self.in_flight = true;
            for (i, p) in self.probes.iter().enumerate() {
                let url = p.url.clone();
                let snap = self.snapshots[i].clone();
                cx.rt.spawn(async move {
                    let client = match reqwest::Client::builder()
                        .timeout(Duration::from_secs(3))
                        .build()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            let mut s = snap.lock().unwrap();
                            s.status = None;
                            s.err = Some(e.to_string());
                            return;
                        }
                    };
                    let result = client.get(&url).send().await;
                    let mut s = snap.lock().unwrap();
                    match result {
                        Ok(r) => {
                            s.status = Some(r.status().as_u16());
                            s.err = None;
                        }
                        Err(e) => {
                            s.status = None;
                            s.err = Some(e.to_string());
                        }
                    }
                });
            }
            self.in_flight = false;
        }

        let rows: Vec<(String, Option<u16>, Option<String>)> = self
            .probes
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let s = self.snapshots[i].lock().unwrap().clone();
                (p.label.clone(), s.status, s.err)
            })
            .collect();

        let down_count = rows
            .iter()
            .filter(|(_, status, err)| match status {
                Some(c) => !(200..400).contains(c),
                None => err.is_some(),
            })
            .count();

        let badge_text = if down_count > 0 {
            Some(format!("{down_count} down"))
        } else {
            None
        };
        let badge = badge_text.as_deref().map(|t| (t, theme.err));

        draw_card(ui, &theme, Some("homelab"), badge, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, (label, status, err)) in rows.iter().enumerate() {
                let (color, text) = match status {
                    Some(c) if (200..400).contains(c) => (theme.ok, format!("{c}")),
                    Some(c) => (theme.err, format!("{c}")),
                    None if err.is_some() => (theme.err, "FAIL".into()),
                    None => (theme.overlay0, "…".into()),
                };
                let (row, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 18.0),
                    egui::Sense::hover(),
                );
                if i % 2 == 1 {
                    let bg = theme.surface1;
                    ui.painter().rect_filled(
                        row,
                        egui::CornerRadius::same(3),
                        egui::Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 100),
                    );
                }
                let painter = ui.painter();
                let dot_pos = row.left_center() + egui::vec2(8.0, 0.0);
                painter.circle_filled(dot_pos, 4.0, color);
                painter.text(
                    row.left_center() + egui::vec2(20.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(13.0),
                    theme.text,
                );
                painter.text(
                    row.right_center() - egui::vec2(8.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    &text,
                    egui::FontId::proportional(FONT_SMALL),
                    color,
                );
            }
        });
        true
    }
}
