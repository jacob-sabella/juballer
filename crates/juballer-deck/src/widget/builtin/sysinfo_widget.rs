//! sysinfo widget — CPU + memory stats as a card with labelled progress bars.
//!
//! Args:
//!   interval_ms : u64 (default 1000) — refresh cadence

use crate::theme::{ease_to, FONT_BODY, FONT_SMALL};
use crate::widget::card::{draw_card, progress_bar};
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::Result;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

pub struct SysinfoWidget {
    interval: Duration,
    last_refresh: Option<Instant>,
    last_frame: Option<Instant>,
    sys: System,
    cpu_target: f32,
    cpu_display: f32,
    mem_target: f32,
    mem_display: f32,
    used_mb: u64,
    total_mb: u64,
}

impl WidgetBuildFromArgs for SysinfoWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let interval_ms = args
            .get("interval_ms")
            .and_then(|v| v.as_integer())
            .map(|i| i.max(200) as u64)
            .unwrap_or(1000);
        let specifics = RefreshKind::new()
            .with_cpu(CpuRefreshKind::new().with_cpu_usage())
            .with_memory(MemoryRefreshKind::new().with_ram());
        let sys = System::new_with_specifics(specifics);
        Ok(Self {
            interval: Duration::from_millis(interval_ms),
            last_refresh: None,
            last_frame: None,
            sys,
            cpu_target: 0.0,
            cpu_display: 0.0,
            mem_target: 0.0,
            mem_display: 0.0,
            used_mb: 0,
            total_mb: 0,
        })
    }
}

impl Widget for SysinfoWidget {
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let now = Instant::now();
        let should_refresh = self
            .last_refresh
            .map(|t| now.duration_since(t) >= self.interval)
            .unwrap_or(true);

        if should_refresh {
            self.sys.refresh_cpu_usage();
            self.sys.refresh_memory();
            self.cpu_target = self.sys.global_cpu_usage();
            self.total_mb = self.sys.total_memory() / 1024 / 1024;
            self.used_mb = self.sys.used_memory() / 1024 / 1024;
            self.mem_target = if self.total_mb > 0 {
                (self.used_mb as f32 / self.total_mb as f32) * 100.0
            } else {
                0.0
            };
            self.last_refresh = Some(now);
        }

        let dt = self
            .last_frame
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(1.0 / 60.0);
        self.last_frame = Some(now);
        ease_to(&mut self.cpu_display, self.cpu_target, dt, 0.25);
        ease_to(&mut self.mem_display, self.mem_target, dt, 0.25);

        let cpu_pct = self.cpu_display;
        let mem_pct = self.mem_display;
        let used = self.used_mb;
        let total = self.total_mb;

        draw_card(ui, &theme, Some("system"), None, |ui| {
            stat_row(
                ui,
                &theme,
                "CPU",
                &format!("{:.1}%", cpu_pct),
                cpu_pct / 100.0,
                theme.accent_alt,
            );
            stat_row(
                ui,
                &theme,
                "MEM",
                &format!("{} / {} MB", used, total),
                mem_pct / 100.0,
                theme.info,
            );
        });
        true
    }
}

fn stat_row(
    ui: &mut egui::Ui,
    theme: &crate::theme::Theme,
    label: &str,
    value: &str,
    frac: f32,
    fill: egui::Color32,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(FONT_SMALL)
                .color(theme.subtext0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).size(FONT_BODY).color(theme.text));
        });
    });
    progress_bar(ui, frac, 6.0, fill, theme.surface1);
}
