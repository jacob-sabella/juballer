//! Deck application shell. Wires juballer-core::App + registries + config + state + bus.

use crate::action::builtin::register_builtins as register_action_builtins;
use crate::action::{Action, ActionRegistry};
use crate::bus::EventBus;
use crate::config::{ConfigTree, DeckPaths};
use crate::state::StateStore;
use crate::tile::{IconRef, TileState};
use crate::widget::builtin::register_builtins as register_widget_builtins;
use crate::widget::WidgetRegistry;
use crate::Result;
use indexmap::IndexMap;
use juballer_core::Color;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

pub struct DeckApp {
    pub paths: DeckPaths,
    pub config: ConfigTree,
    pub state: StateStore,
    pub bus: EventBus,
    pub actions: ActionRegistry,
    pub widgets: WidgetRegistry,
    pub rt: tokio::runtime::Handle,

    /// Active page instance: bound actions indexed by PHYSICAL (row, col) in the 0..4 grid.
    /// Rebuilt whenever scroll offset or the active page changes.
    pub bound_actions: HashMap<(u8, u8), BoundAction>,
    /// Logical button grid for the active page, indexed by logical (row, col).
    /// Holds one `BoundAction` per declared button regardless of whether it's currently
    /// visible. When scrolling, a fresh `BoundAction` is cloned into `bound_actions` for
    /// the physical cell that maps onto each logical position.
    pub logical_buttons: HashMap<(u8, u8), LogicalButton>,
    /// Logical grid dimensions (>= 4).
    pub logical_rows: u8,
    pub logical_cols: u8,
    /// Pinned logical row indices — they render at physical row == logical row and do
    /// not participate in scrolling.
    pub pinned_rows: Vec<u8>,
    pub pinned_cols: Vec<u8>,
    /// Current scroll offsets. Index into the compacted "unpinned logical rows/cols"
    /// list — `scroll_row = 0` shows the first unpinned logical rows, etc.
    pub scroll_row: u8,
    pub scroll_col: u8,
    /// Active top-region widget instances keyed by pane id (matches config pane name).
    pub active_widgets: std::collections::HashMap<String, Box<dyn crate::widget::Widget>>,
    /// Tile state per cell
    pub tiles: [TileState; 16],
    /// Active page name
    pub active_page: String,
    /// Layout pane name interner for current page (kept alive while page is active)
    pub active_pane_interner: HashMap<String, &'static str>,
    /// egui-on-wgpu overlay for tile icon+label rendering and top-region widgets.
    pub egui_overlay: juballer_egui::EguiOverlay,
    /// Image loader/cache for IconRef::Path and config image icons.
    pub icon_loader: crate::icon_loader::IconLoader,
    /// Stack of previously-active pages, pushed on page switch / cycle. Pop on page_back.
    pub page_history: Vec<String>,
    /// Per-frame bus subscription used by the deck loop to react to deck.* + widget.action_request.
    pub deck_bus_rx: tokio::sync::broadcast::Receiver<crate::bus::Event>,
    /// Plugin host (owns spawned plugin processes + UDS connections). `None` until wired.
    /// Wrapped in an `Arc<Mutex<_>>` so the editor server can call `restart_one` while
    /// the deck itself holds the host as long-lived state.
    pub plugin_host: Option<std::sync::Arc<tokio::sync::Mutex<crate::plugin::host::PluginHost>>>,
    /// Optional sender the render loop uses to publish key_preview events to the editor WS.
    pub editor_event_tx: Option<tokio::sync::broadcast::Sender<crate::editor::server::EditorEvent>>,
    /// Pending top-region layout to apply on the next frame. `None` = no change,
    /// `Some(None)` = clear top region, `Some(Some(node))` = install layout.
    pub pending_top_layout: Option<Option<juballer_core::layout::Node>>,
    /// Pane names of the most recently applied top layout (empty if cleared). Used by tests
    /// to observe layout swaps without driving a real GPU.
    pub last_applied_top_pane_names: Vec<String>,
    /// Per-pane structured view trees pushed by plugins via `Message::WidgetViewUpdate`.
    /// Consumed by the `dynamic` widget. Shared with the plugin host's read tasks.
    pub view_trees: Arc<RwLock<HashMap<String, juballer_deck_protocol::view::ViewNode>>>,
    /// Logical name → (logical_row, logical_col) for named buttons on the active page.
    /// Rebuilt each time `bind_active_page` runs. Plugins use `Message::TileSetByName`
    /// to target tiles by name without caring about scroll or page layout.
    pub named_tiles: HashMap<String, (u8, u8)>,
    /// Per-name, plugin-supplied overrides applied after the physical binding fills
    /// in config-default icon/label/state_color. Shared with the plugin host's read
    /// tasks so plugin messages can mutate these without the app lock.
    pub named_tile_overrides: Arc<Mutex<HashMap<String, NamedTileOverride>>>,
    /// Active visual theme (mocha/latte). Widgets read from this rather than hard-coding.
    pub theme: crate::theme::Theme,
    /// Per-tile custom WGSL shader pipeline cache.
    pub shader_cache: crate::shader::ShaderPipelineCache,
    /// Per-tile video source registry (v4l2 backends etc.).
    pub video_registry: crate::video::VideoRegistry,
    /// Wall-clock origin used to compute `Uniforms.time`.
    pub boot_instant: std::time::Instant,
    /// Previous frame's wall time; used to compute `Uniforms.delta_time`.
    pub last_frame_instant: Option<std::time::Instant>,
    /// Master-chord gesture state: instant the top-right corner went down (if held).
    pub master_tr_down: Option<std::time::Instant>,
    /// Master-chord gesture state: instant the bottom-left corner went down (if held).
    pub master_bl_down: Option<std::time::Instant>,
}

pub struct BoundAction {
    pub binding_id: String,
    pub action: Box<dyn Action>,
    pub icon: Option<String>,
    pub label: Option<String>,
}

/// Plugin-supplied override for a named tile. Only set fields overwrite the
/// config default; unset fields leave the config icon/label/state_color visible.
#[derive(Debug, Clone, Default)]
pub struct NamedTileOverride {
    pub icon: Option<String>,
    pub label: Option<String>,
    pub state_color: Option<Color>,
}

/// Per-logical-cell config cached so scrolling can rebuild `BoundAction` instances without
/// re-parsing the page TOML or reinterpreting `env` each scroll tick.
pub struct LogicalButton {
    /// Interpolated TOML args.
    pub args: toml::Table,
    pub action_name: String,
    pub icon: Option<String>,
    pub label: Option<String>,
    pub shader: Option<crate::tile::TileShaderSource>,
    /// Optional stable logical name for plugin-targeted updates.
    pub name: Option<String>,
}

impl DeckApp {
    pub fn bootstrap(paths: DeckPaths, rt: tokio::runtime::Handle) -> Result<Self> {
        let config = ConfigTree::load(&paths)?;
        let state = StateStore::open(paths.state_toml.clone())?;

        let mut actions = ActionRegistry::new();
        register_action_builtins(&mut actions);

        let mut widgets = WidgetRegistry::new();
        register_widget_builtins(&mut widgets);

        let active_profile = config.active_profile()?;
        let active_page = state
            .last_active_page()
            .unwrap_or(&active_profile.meta.default_page)
            .to_string();

        let active_profile_name = config.deck.active_profile.clone();
        let assets_root = paths.profile_assets(&active_profile_name);
        let icon_loader = crate::icon_loader::IconLoader::new(assets_root);

        let bus = EventBus::default();
        let deck_bus_rx = bus.subscribe();
        let theme =
            crate::theme::Theme::from_name(config.deck.render.theme.as_deref().unwrap_or("mocha"));
        let mut app = Self {
            paths,
            config: config.clone(),
            state,
            bus,
            actions,
            widgets,
            rt,
            bound_actions: HashMap::new(),
            logical_buttons: HashMap::new(),
            logical_rows: 4,
            logical_cols: 4,
            pinned_rows: Vec::new(),
            pinned_cols: Vec::new(),
            scroll_row: 0,
            scroll_col: 0,
            active_widgets: std::collections::HashMap::new(),
            tiles: std::array::from_fn(|_| TileState::default()),
            active_page,
            active_pane_interner: HashMap::new(),
            egui_overlay: juballer_egui::EguiOverlay::new(),
            icon_loader,
            page_history: Vec::new(),
            deck_bus_rx,
            plugin_host: None,
            editor_event_tx: None,
            pending_top_layout: None,
            last_applied_top_pane_names: Vec::new(),
            view_trees: Arc::new(RwLock::new(HashMap::new())),
            named_tiles: HashMap::new(),
            named_tile_overrides: Arc::new(Mutex::new(HashMap::new())),
            theme,
            shader_cache: crate::shader::ShaderPipelineCache::new(),
            video_registry: crate::video::VideoRegistry::new(),
            boot_instant: std::time::Instant::now(),
            last_frame_instant: None,
            master_tr_down: None,
            master_bl_down: None,
        };
        app.bind_active_page()?;
        app.queue_top_layout_for_active_page();
        Ok(app)
    }

    /// Parse the active page TOML into `logical_buttons` + dims + pins, restore saved
    /// scroll offset, then rebuild the physical 4x4 binding.
    pub fn bind_active_page(&mut self) -> Result<()> {
        let profile_name = self.config.deck.active_profile.clone();
        let page = self.config.lookup_page(&self.active_page).ok_or_else(|| {
            crate::Error::Config(format!(
                "active page {} not found (profile '{}' or plugin pages)",
                self.active_page, profile_name
            ))
        })?;

        // Capture remembered scroll offset BEFORE mutating self (lookup_page borrows).
        let logical_rows = page.meta.logical_rows.max(4);
        let logical_cols = page.meta.logical_cols.max(4);
        let pinned_rows: Vec<u8> = page
            .meta
            .pinned_rows
            .iter()
            .copied()
            .filter(|&r| r < logical_rows)
            .collect();
        let pinned_cols: Vec<u8> = page
            .meta
            .pinned_cols
            .iter()
            .copied()
            .filter(|&c| c < logical_cols)
            .collect();

        // Build logical button grid.
        let env = self.merged_env();
        let mut logical_buttons: HashMap<(u8, u8), LogicalButton> = HashMap::new();
        for btn in &page.buttons {
            if btn.row >= logical_rows || btn.col >= logical_cols {
                tracing::warn!(
                    "button (row={}, col={}) out of logical range ({}x{}), skipping",
                    btn.row,
                    btn.col,
                    logical_rows,
                    logical_cols
                );
                continue;
            }
            let mut args = btn.args.clone();
            interp_table(&mut args, &env);
            let shader = btn.shader.as_ref().map(|cfg| match cfg {
                crate::config::TileShaderCfg::Wgsl { wgsl } => {
                    crate::tile::TileShaderSource::CustomShader {
                        wgsl_path: std::path::PathBuf::from(wgsl),
                        params: std::collections::HashMap::new(),
                    }
                }
                crate::config::TileShaderCfg::Video { video } => {
                    crate::tile::TileShaderSource::Video { uri: video.clone() }
                }
            });
            logical_buttons.insert(
                (btn.row, btn.col),
                LogicalButton {
                    args,
                    action_name: btn.action.clone(),
                    icon: btn.icon.clone(),
                    label: btn.label.clone(),
                    shader,
                    name: btn.name.clone(),
                },
            );
        }

        // Rebuild named_tiles map from the (new) logical grid.
        let mut named_tiles: HashMap<String, (u8, u8)> = HashMap::new();
        for btn in &page.buttons {
            if btn.row >= logical_rows || btn.col >= logical_cols {
                continue;
            }
            if let Some(n) = &btn.name {
                named_tiles.insert(n.clone(), (btn.row, btn.col));
            }
        }

        // Build top-pane widget instances.
        let top_panes = page.top_panes.clone();
        self.active_widgets.clear();
        for (pane_id, binding) in &top_panes {
            let mut args = binding.args.clone();
            interp_table(&mut args, &env);
            match self.widgets.build(&binding.widget, &args) {
                Ok(w) => {
                    self.active_widgets.insert(pane_id.clone(), w);
                }
                Err(e) => {
                    tracing::warn!("widget {} ({}): {}", pane_id, binding.widget, e);
                }
            }
        }

        // Commit logical state.
        self.logical_buttons = logical_buttons;
        self.logical_rows = logical_rows;
        self.logical_cols = logical_cols;
        self.pinned_rows = pinned_rows;
        self.pinned_cols = pinned_cols;
        self.named_tiles = named_tiles;

        // Restore scroll offset from state, if present.
        let (saved_r, saved_c) = self.load_scroll_offset();
        self.scroll_row = saved_r;
        self.scroll_col = saved_c;
        self.clamp_scroll();

        self.rebuild_physical_binding()?;
        Ok(())
    }

    /// Rebuild the `bound_actions` + `tiles` for the current scroll offset, mapping
    /// logical cells into the physical 4x4 grid. Pinned rows/cols keep their logical
    /// index at the matching physical index; the remaining physical cells walk through
    /// the unpinned logical rows/cols, skewed by `scroll_row`/`scroll_col`.
    pub fn rebuild_physical_binding(&mut self) -> Result<()> {
        self.bound_actions.clear();
        self.tiles = std::array::from_fn(|_| TileState::default());

        let unpinned_log_rows: Vec<u8> = (0..self.logical_rows)
            .filter(|r| !self.pinned_rows.contains(r))
            .collect();
        let unpinned_log_cols: Vec<u8> = (0..self.logical_cols)
            .filter(|c| !self.pinned_cols.contains(c))
            .collect();
        let unpinned_phys_rows: Vec<u8> =
            (0..4u8).filter(|r| !self.pinned_rows.contains(r)).collect();
        let unpinned_phys_cols: Vec<u8> =
            (0..4u8).filter(|c| !self.pinned_cols.contains(c)).collect();

        for phys_r in 0..4u8 {
            for phys_c in 0..4u8 {
                let Some((log_r, log_c)) = physical_to_logical(
                    phys_r,
                    phys_c,
                    &self.pinned_rows,
                    &self.pinned_cols,
                    &unpinned_log_rows,
                    &unpinned_log_cols,
                    &unpinned_phys_rows,
                    &unpinned_phys_cols,
                    self.scroll_row,
                    self.scroll_col,
                ) else {
                    continue;
                };
                let Some(btn) = self.logical_buttons.get(&(log_r, log_c)) else {
                    continue;
                };
                let action = match self.actions.build(&btn.action_name, &btn.args) {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::warn!(
                            "bind skip ({},{}) {}: {}",
                            log_r,
                            log_c,
                            btn.action_name,
                            e
                        );
                        continue;
                    }
                };
                let binding_id = format!("{}:{},{}", self.active_page, log_r, log_c);
                self.bound_actions.insert(
                    (phys_r, phys_c),
                    BoundAction {
                        binding_id,
                        action,
                        icon: btn.icon.clone(),
                        label: btn.label.clone(),
                    },
                );
                if let Some(src) = &btn.shader {
                    self.tiles[(phys_r as usize) * 4 + phys_c as usize].shader = Some(src.clone());
                }
            }
        }
        Ok(())
    }

    /// Clamp `scroll_row`/`scroll_col` so the visible 4x4 window stays inside the
    /// unpinned logical range. If there's nothing to scroll (logical dims are 4x4 or
    /// all extra rows are pinned), the offset pins to 0.
    pub fn clamp_scroll(&mut self) {
        let unpinned_log_rows = (self.logical_rows as usize)
            .saturating_sub(count_in_range(&self.pinned_rows, self.logical_rows) as usize);
        let unpinned_phys_rows =
            4usize.saturating_sub(count_in_range(&self.pinned_rows, 4) as usize);
        let max_scroll_row = unpinned_log_rows.saturating_sub(unpinned_phys_rows) as u8;
        self.scroll_row = self.scroll_row.min(max_scroll_row);

        let unpinned_log_cols = (self.logical_cols as usize)
            .saturating_sub(count_in_range(&self.pinned_cols, self.logical_cols) as usize);
        let unpinned_phys_cols =
            4usize.saturating_sub(count_in_range(&self.pinned_cols, 4) as usize);
        let max_scroll_col = unpinned_log_cols.saturating_sub(unpinned_phys_cols) as u8;
        self.scroll_col = self.scroll_col.min(max_scroll_col);
    }

    /// Key used to persist per-page scroll offset in the state store.
    pub fn scroll_offset_key(&self) -> String {
        format!(
            "scroll_offset:{}:{}",
            self.config.deck.active_profile, self.active_page
        )
    }

    /// Read a previously saved scroll offset for the active page (profile + page).
    /// Defaults to (0, 0) if no entry exists.
    pub fn load_scroll_offset(&self) -> (u8, u8) {
        let key = self.scroll_offset_key();
        let Some(v) = self.state.binding(&key) else {
            return (0, 0);
        };
        let r = v.get("row").and_then(|x| x.as_u64()).unwrap_or(0) as u8;
        let c = v.get("col").and_then(|x| x.as_u64()).unwrap_or(0) as u8;
        (r, c)
    }

    /// Persist the current scroll offset to the state store. Callers typically flush
    /// via the normal state.flush cadence.
    pub fn save_scroll_offset(&mut self) {
        let key = self.scroll_offset_key();
        self.state.set_binding(
            key,
            serde_json::json!({
                "row": self.scroll_row,
                "col": self.scroll_col,
            }),
        );
    }

    pub fn merged_env(&self) -> IndexMap<String, String> {
        let mut env = IndexMap::new();
        if let Ok(profile) = self.config.active_profile() {
            for (k, v) in &profile.meta.env {
                env.insert(k.clone(), v.clone());
            }
        }
        env
    }

    /// Queue the top-region layout for the active page. Sets `pending_top_layout` so the
    /// next render frame installs it; updates `last_applied_top_pane_names` for observability.
    /// If the active page has no `top` declaration, the top region is cleared.
    pub fn queue_top_layout_for_active_page(&mut self) {
        let layout_cfg = self
            .config
            .lookup_page(&self.active_page)
            .and_then(|page| page.top.clone());
        match layout_cfg {
            Some(cfg) => {
                match crate::layout_convert::convert(&cfg, &mut self.active_pane_interner) {
                    Ok(out) => {
                        self.last_applied_top_pane_names = out.pane_names;
                        self.pending_top_layout = Some(Some(out.root));
                    }
                    Err(e) => {
                        tracing::warn!("top layout build failed: {}", e);
                        self.last_applied_top_pane_names.clear();
                        self.pending_top_layout = Some(None);
                    }
                }
            }
            None => {
                self.last_applied_top_pane_names.clear();
                self.pending_top_layout = Some(None);
            }
        }
    }

    /// Locate the currently-visible physical cell of a named button, if any.
    /// Returns `None` if the name is unknown for the active page or the named
    /// logical cell is scrolled out of the physical 4x4 window.
    pub fn physical_of_named(&self, name: &str) -> Option<(u8, u8)> {
        let (log_r, log_c) = *self.named_tiles.get(name)?;
        let unpinned_log_rows: Vec<u8> = (0..self.logical_rows)
            .filter(|r| !self.pinned_rows.contains(r))
            .collect();
        let unpinned_log_cols: Vec<u8> = (0..self.logical_cols)
            .filter(|c| !self.pinned_cols.contains(c))
            .collect();
        let unpinned_phys_rows: Vec<u8> =
            (0..4u8).filter(|r| !self.pinned_rows.contains(r)).collect();
        let unpinned_phys_cols: Vec<u8> =
            (0..4u8).filter(|c| !self.pinned_cols.contains(c)).collect();

        let phys_r = if self.pinned_rows.contains(&log_r) {
            if log_r < 4 {
                log_r
            } else {
                return None;
            }
        } else {
            let idx = unpinned_log_rows.iter().position(|&r| r == log_r)?;
            let target = idx.checked_sub(self.scroll_row as usize)?;
            *unpinned_phys_rows.get(target)?
        };
        let phys_c = if self.pinned_cols.contains(&log_c) {
            if log_c < 4 {
                log_c
            } else {
                return None;
            }
        } else {
            let idx = unpinned_log_cols.iter().position(|&c| c == log_c)?;
            let target = idx.checked_sub(self.scroll_col as usize)?;
            *unpinned_phys_cols.get(target)?
        };
        Some((phys_r, phys_c))
    }

    /// Apply all queued plugin overrides to `self.tiles`. Called per-frame from the render
    /// loop before drawing so a scroll or page switch immediately reflects the latest
    /// plugin-supplied state without a round trip through the plugin.
    pub fn apply_named_tile_overrides(&mut self) {
        let overrides = match self.named_tile_overrides.lock() {
            Ok(g) => g.clone(),
            Err(_) => return,
        };
        for (name, ov) in overrides.iter() {
            let Some((phys_r, phys_c)) = self.physical_of_named(name) else {
                continue;
            };
            let idx = (phys_r as usize) * 4 + phys_c as usize;
            let tile = &mut self.tiles[idx];
            if let Some(icon) = &ov.icon {
                tile.icon = Some(IconRef::Emoji(icon.clone()));
            }
            if let Some(label) = &ov.label {
                tile.label = Some(label.clone());
            }
            if let Some(color) = ov.state_color {
                tile.state_color = Some(color);
            }
        }
    }

    /// Render bg color from deck.toml; falls back to the active theme's `base`.
    pub fn bg_color(&self) -> Color {
        if let Some(s) = self.config.deck.render.bg.as_deref() {
            if let Some(c) = parse_hex_color(s) {
                return c;
            }
        }
        let t = self.theme.base;
        Color::rgba(t.r(), t.g(), t.b(), t.a())
    }
}

/// Count how many values in `xs` are strictly less than `limit`.
fn count_in_range(xs: &[u8], limit: u8) -> u8 {
    xs.iter().filter(|&&v| v < limit).count() as u8
}

/// Map a physical (4x4) cell into its logical (logical_rows x logical_cols) coordinates,
/// accounting for pinned rows/cols and the current scroll offset. Returns `None` if the
/// physical cell has no logical mapping (e.g. `pinned_rows = [5]` with logical_rows=8 but
/// no physical row at index 5 — this can't happen for valid configs, but we gracefully
/// skip if pinning data is inconsistent).
#[allow(clippy::too_many_arguments)]
fn physical_to_logical(
    phys_r: u8,
    phys_c: u8,
    pinned_rows: &[u8],
    pinned_cols: &[u8],
    unpinned_log_rows: &[u8],
    unpinned_log_cols: &[u8],
    unpinned_phys_rows: &[u8],
    unpinned_phys_cols: &[u8],
    scroll_row: u8,
    scroll_col: u8,
) -> Option<(u8, u8)> {
    let log_r = if pinned_rows.contains(&phys_r) {
        phys_r
    } else {
        let idx = unpinned_phys_rows.iter().position(|&r| r == phys_r)?;
        let target = idx + scroll_row as usize;
        *unpinned_log_rows.get(target)?
    };
    let log_c = if pinned_cols.contains(&phys_c) {
        phys_c
    } else {
        let idx = unpinned_phys_cols.iter().position(|&c| c == phys_c)?;
        let target = idx + scroll_col as usize;
        *unpinned_log_cols.get(target)?
    };
    Some((log_r, log_c))
}

fn interp_table(table: &mut toml::Table, env: &IndexMap<String, String>) {
    let mut h: std::collections::HashMap<String, String> =
        env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in std::env::vars() {
        h.entry(k).or_insert(v);
    }
    interp_value_in_place(table, &h);
}

fn interp_value_in_place(t: &mut toml::Table, env: &std::collections::HashMap<String, String>) {
    for (_, v) in t.iter_mut() {
        interp_one(v, env);
    }
}

fn interp_one(v: &mut toml::Value, env: &std::collections::HashMap<String, String>) {
    match v {
        toml::Value::String(s) => *s = crate::config::interpolate::interpolate(s, env),
        toml::Value::Table(t) => interp_value_in_place(t, env),
        toml::Value::Array(a) => {
            for x in a.iter_mut() {
                interp_one(x, env);
            }
        }
        _ => {}
    }
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    let bytes = match s.len() {
        6 => hex::decode(s).ok()?,
        8 => hex::decode(s).ok()?,
        _ => return None,
    };
    let a = if bytes.len() == 4 { bytes[3] } else { 0xff };
    Some(Color::rgba(bytes[0], bytes[1], bytes[2], a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &std::path::Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn bootstrap_binds_shell_action() {
        let dir = tempdir().unwrap();
        let paths = DeckPaths::from_root(dir.path().to_path_buf());
        write(
            &paths.deck_toml,
            r##"
version = 1
active_profile = "p"

[editor]
bind = "127.0.0.1:7373"

[render]

[log]
level = "info"
"##,
        );
        write(
            &paths.profile_meta_toml("p"),
            r##"
name = "p"
default_page = "home"
pages = ["home"]
"##,
        );
        write(
            &paths.profile_page_toml("p", "home"),
            r##"
[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "shell.run"
args = { cmd = "echo hi" }
"##,
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let app = DeckApp::bootstrap(paths, rt.handle().clone()).unwrap();
        assert!(app.bound_actions.contains_key(&(0, 0)));
        assert_eq!(app.bound_actions[&(0, 0)].binding_id, "home:0,0");
    }
}
