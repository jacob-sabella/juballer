//! plugin_proxy widget — renders declarative content from a plugin via bus messages.
//!
//! Subscribes to `plugin.<pane_id>.widget_set` topic. The deck's plugin host translates
//! plugin-sent WidgetSet messages into bus events on this topic.
//!
//! Args:
//!   topic_override : string (optional) — defaults to "plugin." + pane_id
//!   title          : string (optional) — card header (default "plugin")

use crate::theme::{FONT_BODY, FONT_HEADER, FONT_LARGE, FONT_SMALL};
use crate::widget::card::draw_card;
use crate::widget::{Widget, WidgetBuildFromArgs, WidgetCx};
use crate::Result;
use tokio::sync::broadcast;

pub struct PluginProxyWidget {
    subscribe_topic: Option<String>,
    title: String,
    rx: Option<broadcast::Receiver<crate::bus::Event>>,
    content: serde_json::Value,
}

impl WidgetBuildFromArgs for PluginProxyWidget {
    fn from_args(args: &toml::Table) -> Result<Self> {
        let subscribe_topic = args
            .get("topic_override")
            .and_then(|v| v.as_str())
            .map(String::from);
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("plugin")
            .to_string();
        Ok(Self {
            subscribe_topic,
            title,
            rx: None,
            content: serde_json::json!({}),
        })
    }
}

impl Widget for PluginProxyWidget {
    fn on_will_appear(&mut self, cx: &mut WidgetCx<'_>) {
        self.rx = Some(cx.bus.subscribe());
    }

    fn on_will_disappear(&mut self, _cx: &mut WidgetCx<'_>) {
        self.rx = None;
    }

    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool {
        let theme = cx.theme;
        let topic = self
            .subscribe_topic
            .clone()
            .unwrap_or_else(|| format!("plugin.{}.widget_set", cx.pane));

        if let Some(rx) = self.rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        if ev.topic == topic {
                            self.content = ev.data;
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

        let content = self.content.clone();
        let title = self.title.clone();
        draw_card(ui, &theme, Some(&title), None, |ui| {
            render_content(ui, &content, &theme);
        });
        true
    }
}

fn render_content(ui: &mut egui::Ui, content: &serde_json::Value, theme: &crate::theme::Theme) {
    let layout = content
        .get("layout")
        .and_then(|v| v.as_str())
        .unwrap_or("vertical");
    let children = content.get("children").and_then(|v| v.as_array());

    let render_children = |ui: &mut egui::Ui, kids: &[serde_json::Value]| {
        for child in kids {
            render_primitive(ui, child, theme);
        }
    };

    match (layout, children) {
        ("horizontal", Some(kids)) => {
            ui.horizontal(|ui| render_children(ui, kids));
        }
        (_, Some(kids)) => {
            ui.vertical(|ui| render_children(ui, kids));
        }
        _ => {
            render_primitive(ui, content, theme);
        }
    }
}

fn render_primitive(ui: &mut egui::Ui, v: &serde_json::Value, theme: &crate::theme::Theme) {
    if let Some(s) = v.get("heading").and_then(|x| x.as_str()) {
        ui.label(egui::RichText::new(s).size(FONT_HEADER).color(theme.text));
        return;
    }
    if let Some(s) = v.get("label").and_then(|x| x.as_str()) {
        ui.label(egui::RichText::new(s).size(FONT_BODY).color(theme.text));
        return;
    }
    if let Some(s) = v.get("big").and_then(|x| x.as_str()) {
        ui.label(egui::RichText::new(s).size(FONT_LARGE).color(theme.text));
        if let Some(small) = v.get("small").and_then(|x| x.as_str()) {
            ui.label(
                egui::RichText::new(small)
                    .size(FONT_SMALL)
                    .color(theme.subtext1),
            );
        }
        return;
    }
    if v.get("spacer").and_then(|x| x.as_bool()).unwrap_or(false) {
        ui.add_space(6.0);
        return;
    }
    if let Some(s) = v.get("badge").and_then(|x| x.as_str()) {
        ui.label(
            egui::RichText::new(s)
                .size(FONT_SMALL)
                .color(theme.accent_alt),
        );
        return;
    }
    if v.is_object() && v.get("layout").is_some() {
        render_content(ui, v, theme);
    }
}
