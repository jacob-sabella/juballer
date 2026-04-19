//! Preset picker overlay — opens when an `open-preset-picker` cell
//! fires, and applies the selected preset to a chosen plugin slot.
//!
//! Layout mirrors [`super::picker`]: 12 paginated tiles + a bottom
//! navigation row (PREV / NEXT / BACK / EXIT). The two pickers don't
//! share an implementation because their tile-press semantics diverge:
//! the config picker re-loads `CarlaState`, this one calls
//! [`super::preset::apply`] without touching `CarlaState`.

use crate::carla::preset::{PresetEntry, PresetLibrary};
use juballer_core::ui::pagination::{Paginator, DEFAULT_TRANSITION_MS};
use std::path::PathBuf;

pub const TILES_PER_PAGE: usize = 12;
pub const NAV_PREV_COL: u8 = 0;
pub const NAV_NEXT_COL: u8 = 1;
pub const NAV_BACK_COL: u8 = 2;
pub const NAV_EXIT_COL: u8 = 3;
pub const NAV_ROW: u8 = 3;

/// Picker state. Holds the (already-filtered) entry list, the active
/// plugin slot to write into, and the current category filter (purely
/// for display in the HUD; the entry list is pre-filtered when
/// [`Self::new_from_library`] is called).
pub struct PresetPickerState {
    pages: Paginator<PresetEntry>,
    target_plugin: Option<u32>,
    category: Option<String>,
}

impl PresetPickerState {
    /// Build a picker for the entire library. Use this when the
    /// triggering cell didn't specify a category.
    pub fn new_all(library: &PresetLibrary, target_plugin: Option<u32>) -> Self {
        Self {
            pages: Paginator::new(library.sorted(), TILES_PER_PAGE),
            target_plugin,
            category: None,
        }
    }

    /// Build a picker scoped to one category (the directory name).
    pub fn new_for_category(
        library: &PresetLibrary,
        category: String,
        target_plugin: Option<u32>,
    ) -> Self {
        let entries = library.by_category(&category);
        Self {
            pages: Paginator::new(entries, TILES_PER_PAGE),
            target_plugin,
            category: Some(category),
        }
    }

    /// Convenience entry point used by the event loop: chooses
    /// `new_for_category` when the cell binding declared one,
    /// otherwise `new_all`.
    pub fn new_from_library(
        library: &PresetLibrary,
        category: Option<String>,
        target_plugin: Option<u32>,
    ) -> Self {
        match category {
            Some(c) => Self::new_for_category(library, c, target_plugin),
            None => Self::new_all(library, target_plugin),
        }
    }

    pub fn target_plugin(&self) -> Option<u32> {
        self.target_plugin
    }

    pub fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }

    pub fn current_entries(&self) -> &[PresetEntry] {
        self.pages.current_items()
    }

    pub fn entry_at_cell(&self, row: u8, col: u8) -> Option<&PresetEntry> {
        if row >= NAV_ROW {
            return None;
        }
        let idx = (row as usize) * 4 + (col as usize);
        self.current_entries().get(idx)
    }

    pub fn next_page(&mut self) -> bool {
        self.pages.next_page(DEFAULT_TRANSITION_MS)
    }

    pub fn prev_page(&mut self) -> bool {
        self.pages.prev_page(DEFAULT_TRANSITION_MS)
    }

    pub fn page_count(&self) -> usize {
        self.pages.page_count()
    }

    pub fn current_page_index(&self) -> usize {
        self.pages.current_page()
    }

    pub fn total(&self) -> usize {
        self.pages.total()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    pub fn tick(&mut self) {
        self.pages.tick();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PresetPickerAction {
    /// Apply the preset at this path to the picker's bound plugin
    /// slot. The caller (the event loop) re-loads the entry from the
    /// library and dispatches via [`super::preset::apply`].
    Apply {
        preset_name: String,
        path: PathBuf,
    },
    Back,
    Exit,
    PagePrev,
    PageNext,
    None,
}

pub fn classify_press(state: &PresetPickerState, row: u8, col: u8) -> PresetPickerAction {
    if row == NAV_ROW {
        return match col {
            c if c == NAV_PREV_COL => PresetPickerAction::PagePrev,
            c if c == NAV_NEXT_COL => PresetPickerAction::PageNext,
            c if c == NAV_BACK_COL => PresetPickerAction::Back,
            c if c == NAV_EXIT_COL => PresetPickerAction::Exit,
            _ => PresetPickerAction::None,
        };
    }
    match state.entry_at_cell(row, col) {
        Some(entry) => PresetPickerAction::Apply {
            preset_name: entry.name(),
            path: entry.path.clone(),
        },
        None => PresetPickerAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_library() -> PresetLibrary {
        let dir = tempfile::tempdir().unwrap().keep();
        for cat in ["cabs", "amps"] {
            let cat_dir = dir.join(cat);
            std::fs::create_dir_all(&cat_dir).unwrap();
            for name in ["A", "B", "C"] {
                let body = format!(
                    "name = \"{cat}-{name}\"\ntarget_plugin = \"X\"\n[[param]]\nname = 0\nvalue = 0.5\n"
                );
                std::fs::write(cat_dir.join(format!("{name}.preset.toml")), body).unwrap();
            }
        }
        PresetLibrary::from_root(&dir)
    }

    #[test]
    fn new_all_paginates_the_full_library_in_alphabetical_order() {
        let lib = make_library();
        let state = PresetPickerState::new_all(&lib, Some(2));
        assert_eq!(state.total(), 6);
        assert_eq!(state.target_plugin(), Some(2));
        let names: Vec<String> = state
            .current_entries()
            .iter()
            .map(PresetEntry::name)
            .collect();
        assert!(names.contains(&"amps-A".to_string()));
        assert!(names.contains(&"cabs-C".to_string()));
    }

    #[test]
    fn new_for_category_filters_by_directory() {
        let lib = make_library();
        let state = PresetPickerState::new_for_category(&lib, "cabs".into(), None);
        assert_eq!(state.total(), 3);
        for entry in state.current_entries() {
            assert_eq!(entry.category.as_deref(), Some("cabs"));
        }
    }

    #[test]
    fn classify_press_routes_nav_row_correctly() {
        let lib = make_library();
        let state = PresetPickerState::new_all(&lib, None);
        assert_eq!(
            classify_press(&state, NAV_ROW, NAV_PREV_COL),
            PresetPickerAction::PagePrev
        );
        assert_eq!(
            classify_press(&state, NAV_ROW, NAV_NEXT_COL),
            PresetPickerAction::PageNext
        );
        assert_eq!(
            classify_press(&state, NAV_ROW, NAV_BACK_COL),
            PresetPickerAction::Back
        );
        assert_eq!(
            classify_press(&state, NAV_ROW, NAV_EXIT_COL),
            PresetPickerAction::Exit
        );
    }

    #[test]
    fn classify_press_returns_apply_for_in_range_tile() {
        let lib = make_library();
        let state = PresetPickerState::new_all(&lib, Some(0));
        let action = classify_press(&state, 0, 0);
        match action {
            PresetPickerAction::Apply { preset_name, .. } => {
                assert!(preset_name.starts_with("amps-") || preset_name.starts_with("cabs-"));
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn empty_category_yields_empty_picker() {
        let lib = make_library();
        let state = PresetPickerState::new_for_category(&lib, "nonexistent".into(), None);
        assert!(state.is_empty());
        assert_eq!(state.total(), 0);
    }

    #[test]
    fn entry_at_cell_returns_none_for_out_of_range_coordinates() {
        let lib = PresetLibrary::default();
        let state = PresetPickerState::new_all(&lib, None);
        assert!(state.entry_at_cell(0, 0).is_none());
        for col in 0..4 {
            assert!(state.entry_at_cell(NAV_ROW, col).is_none());
        }
    }

    #[test]
    fn nav_constants_match_documented_layout() {
        assert_eq!((NAV_ROW, NAV_PREV_COL), (3, 0));
        assert_eq!((NAV_ROW, NAV_NEXT_COL), (3, 1));
        assert_eq!((NAV_ROW, NAV_BACK_COL), (3, 2));
        assert_eq!((NAV_ROW, NAV_EXIT_COL), (3, 3));
    }
}
