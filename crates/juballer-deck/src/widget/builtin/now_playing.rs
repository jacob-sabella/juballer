//! now_playing widget — displays current media title + artist via playerctl.
//!
//! Args:
//!   interval_ms : u64 (default 2000)

use crate::theme::{FONT_BODY, FONT_HEADER};
use crate::widget::card::draw_card;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::Result;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default, Clone)]
struct NowState {
    title: Option<String>,
    artist: Option<String>,
    status: Option<String>,
    updated_at: Option<Instant>,
}

pub struct NowPlayingWidget {
    interval: Duration,
    state: Arc<Mutex<NowState>>,
    fetch_in_flight: bool,
    last_fired: Option<Instant>,
    /// Content fingerprint of the last frame we faded in. The fade
    /// re-triggers only when the displayed strings actually change so
    /// the card doesn't flash on every poll.
    last_content_key: Option<(Option<String>, Option<String>, Option<String>)>,
    fade_alpha: f32,
}

impl WidgetBuildFromArgs for NowPlayingWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let interval_ms = args
            .get("interval_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(500) as u64)
            .unwrap_or(2000);
        Ok(Self {
            interval: Duration::from_millis(interval_ms),
            state: Arc::new(Mutex::new(NowState::default())),
            fetch_in_flight: false,
            last_fired: None,
            last_content_key: None,
            fade_alpha: 1.0,
        })
    }
}

impl Widget for NowPlayingWidget {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let now = Instant::now();
        let should_fire = !self.fetch_in_flight
            && self
                .last_fired
                .map(|t| now.duration_since(t) >= self.interval)
                .unwrap_or(true);

        if should_fire {
            self.last_fired = Some(now);
            self.fetch_in_flight = true;
            let state = self.state.clone();
            cx.rt.spawn(async move {
                // Single playerctl invocation that picks one player
                // (first by playerctl's order) and returns all fields
                // in one shot. Polling metadata/metadata/status as
                // separate calls can land on *different* players per
                // call when several MPRIS sources are live, which
                // makes the widget flicker.
                //
                // `--format "{{status}}\u001f{{title}}\u001f{{artist}}"`
                // emits one line with US (0x1f) field separators —
                // safer than spaces because titles can contain anything.
                let out = tokio::process::Command::new("playerctl")
                    .args([
                        "metadata",
                        "--format",
                        "{{status}}\u{1f}{{title}}\u{1f}{{artist}}",
                    ])
                    .output()
                    .await;
                let (status, title, artist) = match out {
                    Ok(o) if o.status.success() => {
                        let s = String::from_utf8_lossy(&o.stdout);
                        let s = s.trim_end_matches('\n');
                        let mut it = s.splitn(3, '\u{1f}');
                        let status = it.next().map(str::to_string);
                        let title = it.next().map(str::to_string);
                        let artist = it.next().map(str::to_string);
                        (status, title, artist)
                    }
                    _ => (None, None, None),
                };
                let mut st = state.lock().unwrap();
                st.title = title;
                st.artist = artist;
                st.status = status;
                st.updated_at = Some(Instant::now());
            });
        }

        let snapshot = self.state.lock().unwrap().clone();
        if snapshot.updated_at.is_some() {
            self.fetch_in_flight = false;
        }

        // Only flash the fade when actual content (title / artist /
        // status) changes; keying off `updated_at` would re-fire on
        // every poll even when the string is identical.
        let content_key = (
            snapshot.title.clone(),
            snapshot.artist.clone(),
            snapshot.status.clone(),
        );
        if self.last_content_key.as_ref() != Some(&content_key) {
            self.last_content_key = Some(content_key);
            self.fade_alpha = 0.15;
        }
        if self.fade_alpha < 1.0 {
            self.fade_alpha = (self.fade_alpha + 1.0 / 60.0 * 4.0).min(1.0);
        }
        let fade_u8 = |c: egui::Color32| {
            egui::Color32::from_rgba_unmultiplied(
                c.r(),
                c.g(),
                c.b(),
                (self.fade_alpha * 255.0) as u8,
            )
        };

        let (badge_text, badge_color) = match snapshot.status.as_deref() {
            Some("Playing") => ("● playing", theme.ok),
            Some("Paused") => ("● paused", theme.warn),
            Some(s) if !s.is_empty() => ("● stopped", theme.overlay1),
            _ => ("", theme.overlay0),
        };
        let badge = if badge_text.is_empty() {
            None
        } else {
            Some((badge_text, badge_color))
        };

        draw_card(ui, &theme, None, badge, |ui| {
            match snapshot.title.as_deref() {
                Some(t) if !t.is_empty() => {
                    let avail = ui.available_width();
                    let truncated = truncate_to_width(t, avail, FONT_HEADER);
                    ui.label(
                        egui::RichText::new(&truncated)
                            .size(FONT_HEADER)
                            .strong()
                            .color(fade_u8(theme.text)),
                    );
                }
                _ => {
                    ui.label(
                        egui::RichText::new("silence")
                            .size(FONT_HEADER)
                            .italics()
                            .color(theme.overlay1),
                    );
                }
            }
            if let Some(a) = snapshot.artist.as_deref() {
                if !a.is_empty() {
                    ui.label(
                        egui::RichText::new(a)
                            .size(FONT_BODY)
                            .color(fade_u8(theme.subtext1)),
                    );
                }
            }
        });
        true
    }
}

fn truncate_to_width(s: &str, width_px: f32, font_size: f32) -> String {
    let approx_char_w = font_size * 0.55;
    let max_chars = ((width_px / approx_char_w).floor() as usize).max(3);
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out = s.chars().take(max_chars - 1).collect::<String>();
        out.push('…');
        out
    }
}
