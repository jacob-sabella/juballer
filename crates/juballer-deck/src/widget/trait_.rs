use crate::bus::EventBus;
use crate::state::StateStore;
use crate::Result;
use indexmap::IndexMap;
use juballer_deck_protocol::view::ViewNode;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct WidgetCx<'a> {
    pub pane: juballer_core::layout::PaneId,
    pub env: &'a IndexMap<String, String>,
    pub bus: &'a EventBus,
    pub state: &'a mut StateStore,
    pub rt: &'a tokio::runtime::Handle,
    pub view_trees: &'a Arc<RwLock<HashMap<String, ViewNode>>>,
    pub theme: crate::theme::Theme,
}

pub trait Widget: Send + 'static {
    fn on_will_appear(&mut self, cx: &mut WidgetCx<'_>) {
        let _ = cx;
    }
    fn on_will_disappear(&mut self, cx: &mut WidgetCx<'_>) {
        let _ = cx;
    }
    /// Render called each frame the widget's pane is visible. Returns `true` to request
    /// immediate redraw (animations).
    fn render(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) -> bool;
}

pub trait WidgetBuildFromArgs: Sized {
    fn from_args(args: &toml::Table) -> Result<Self>;
}
