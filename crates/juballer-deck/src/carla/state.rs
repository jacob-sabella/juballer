//! Pure state for Carla mode: parameter cache, sub-page navigation,
//! HUD breadcrumbs. No I/O, no rendering, no winit — those live in
//! [`super::mod`] and [`super::render`].
//!
//! The split exists because every cell-press behaviour depends on the
//! *current* parameter value (toggle flips it, bump-up adds to it,
//! carousel walks an index over it). Caching that locally — keyed by
//! the resolved (plugin_id, param_id) tuple — keeps dispatch a pure
//! function and avoids a round-trip to Carla on every press.

use crate::carla::config::{Configuration, Page, ParamRef, PluginRef};
use juballer_core::ui::pagination::Paginator;
use std::collections::HashMap;

/// Local cache of the most recently written parameter value for each
/// (plugin_id, param_id) tuple. Phase 1 seeds this from cell `value`
/// fields and updates it on every successful dispatch; Phase 2 will
/// also keep it in sync with Carla's `/Carla/register` push stream.
#[derive(Debug, Default, Clone)]
pub struct ParamValueCache {
    inner: HashMap<(u32, u32), f32>,
}

impl ParamValueCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, plugin: u32, param: u32) -> Option<f32> {
        self.inner.get(&(plugin, param)).copied()
    }

    /// Store / overwrite the cached value. Returns the previous value
    /// (if any) so callers can detect first-write vs update.
    pub fn set(&mut self, plugin: u32, param: u32, value: f32) -> Option<f32> {
        self.inner.insert((plugin, param), value)
    }

    /// Clear all cached values — use on config switch so a new
    /// configuration starts with a clean slate.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// What the HUD shows in the top region after the most recent dispatch.
/// `None` until any cell fires.
#[derive(Debug, Clone, PartialEq)]
pub struct LastTouched {
    pub plugin: PluginRef,
    pub param: ParamRef,
    pub value: f32,
}

/// All Carla-mode runtime state. Wraps the active configuration plus
/// the per-instance derived bits (paginator, cache, breadcrumbs).
pub struct CarlaState {
    config: Configuration,
    pages: Paginator<Page>,
    cache: ParamValueCache,
    last_touched: Option<LastTouched>,
}

impl CarlaState {
    /// Build a fresh state from a freshly-loaded configuration. The
    /// paginator runs at one page per "page" in the config (no inner
    /// pagination of cells; cells are placed by their (row, col) and
    /// padded out to 12 visible slots in the renderer).
    pub fn new(config: Configuration) -> Self {
        let pages = Paginator::new(config.pages.clone(), 1);
        Self {
            config,
            pages,
            cache: ParamValueCache::new(),
            last_touched: None,
        }
    }

    pub fn config(&self) -> &Configuration {
        &self.config
    }

    pub fn cache(&self) -> &ParamValueCache {
        &self.cache
    }

    pub fn cache_mut(&mut self) -> &mut ParamValueCache {
        &mut self.cache
    }

    pub fn last_touched(&self) -> Option<&LastTouched> {
        self.last_touched.as_ref()
    }

    pub fn set_last_touched(&mut self, plugin: PluginRef, param: ParamRef, value: f32) {
        self.last_touched = Some(LastTouched {
            plugin,
            param,
            value,
        });
    }

    /// Active page slice (always exactly one page; first element).
    pub fn active_page(&self) -> Option<&Page> {
        self.pages.current_items().first()
    }

    /// Total number of sub-pages in the active configuration. At least 1.
    pub fn page_count(&self) -> usize {
        self.pages.page_count().max(1)
    }

    pub fn current_page_index(&self) -> usize {
        self.pages.current_page()
    }

    pub fn next_page(&mut self) -> bool {
        self.pages
            .next_page(juballer_core::ui::pagination::DEFAULT_TRANSITION_MS)
    }

    pub fn prev_page(&mut self) -> bool {
        self.pages
            .prev_page(juballer_core::ui::pagination::DEFAULT_TRANSITION_MS)
    }

    /// Advance any in-flight page transition. Safe to call every frame.
    pub fn tick(&mut self) {
        self.pages.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carla::config::{Action, ActionMode, Cell, Page};

    fn cell_with_tap(row: u8, col: u8) -> Cell {
        Cell {
            row,
            col,
            label: None,
            tap: Some(Action {
                plugin: PluginRef::Index(0),
                param: Some(PluginRef::Index(0)),
                mode: ActionMode::Set,
                step: None,
                min: None,
                max: None,
                value: Some(0.5),
                on_value: None,
                off_value: None,
                values: None,
                value_labels: None,
                preset: None,
                category: None,
            }),
            hold: None,
            display: None,
        }
    }

    fn cfg_with_pages(pages: Vec<Page>) -> Configuration {
        Configuration {
            name: Some("Test".into()),
            description: None,
            carla: Default::default(),
            pages,
        }
    }

    #[test]
    fn param_cache_get_set_round_trip_and_clear() {
        let mut c = ParamValueCache::new();
        assert!(c.is_empty());
        assert_eq!(c.set(1, 2, 0.5), None);
        assert_eq!(c.get(1, 2), Some(0.5));
        assert_eq!(c.set(1, 2, 0.75), Some(0.5));
        assert_eq!(c.len(), 1);
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn state_active_page_returns_none_when_config_has_no_pages() {
        let s = CarlaState::new(cfg_with_pages(vec![]));
        assert_eq!(s.page_count(), 1, "empty configs still report one page");
        assert!(s.active_page().is_none());
    }

    #[test]
    fn state_next_advances_active_page_by_one_in_a_multi_page_config() {
        // Wrap behaviour is exercised at the Paginator level; here we
        // just confirm CarlaState plumbs next_page through to the
        // paginator so the active page actually changes.
        let pages = vec![
            Page {
                title: Some("A".into()),
                cells: vec![cell_with_tap(0, 0)],
            },
            Page {
                title: Some("B".into()),
                cells: vec![cell_with_tap(0, 0)],
            },
            Page {
                title: Some("C".into()),
                cells: vec![cell_with_tap(0, 0)],
            },
        ];
        let mut s = CarlaState::new(cfg_with_pages(pages));
        assert_eq!(s.current_page_index(), 0);
        assert_eq!(s.active_page().unwrap().title.as_deref(), Some("A"));
        assert!(s.next_page());
        assert_eq!(s.current_page_index(), 1);
        assert_eq!(s.active_page().unwrap().title.as_deref(), Some("B"));
    }

    #[test]
    fn state_set_last_touched_records_breadcrumb_for_hud() {
        let mut s = CarlaState::new(cfg_with_pages(vec![]));
        assert!(s.last_touched().is_none());
        s.set_last_touched(PluginRef::Index(7), PluginRef::Index(3), 0.42);
        let lt = s.last_touched().unwrap();
        assert_eq!(lt.plugin, PluginRef::Index(7));
        assert_eq!(lt.value, 0.42);
    }
}
