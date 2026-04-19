//! Config picker overlay for Carla mode.
//!
//! When the operator presses the bottom-row CONFIGS cell on the active
//! grid, [`super::run`] swaps in a [`PickerState`] and renders this
//! module's tile grid. Each cell shows one configuration TOML found
//! under `~/.config/juballer/carla/configs/`; pressing it returns a
//! [`PickerAction::Activate`] outcome which the caller honours by
//! tearing down the current `CarlaState` and rebuilding it from the
//! selected file (no re-exec — the OSC client persists).
//!
//! Layout mirrors the rhythm picker:
//!
//! ```text
//! cells 0..=11 (rows 0-2): one configuration per tile, paginated
//! cell (3,0)  PAGE-PREV   cell (3,1) PAGE-NEXT
//! cell (3,2)  BACK        cell (3,3) EXIT
//! ```

use crate::carla::config::Configuration;
use juballer_core::ui::pagination::{Paginator, DEFAULT_TRANSITION_MS};
use std::path::{Path, PathBuf};

pub const TILES_PER_PAGE: usize = 12;
pub const NAV_PREV_COL: u8 = 0;
pub const NAV_NEXT_COL: u8 = 1;
pub const NAV_BACK_COL: u8 = 2;
pub const NAV_EXIT_COL: u8 = 3;
pub const NAV_ROW: u8 = 3;

/// One scanned configuration file. Holds just enough metadata to draw
/// the tile and identify the file for re-loading on activation;
/// configurations are *not* fully parsed during scan so a malformed
/// peer doesn't take the whole picker down.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigEntry {
    pub path: PathBuf,
    pub name: String,
    pub description: Option<String>,
}

impl ConfigEntry {
    /// Parse just enough of `path` to populate the entry. Files that
    /// fail to parse / validate become `None`; the caller (the picker
    /// scanner) drops them and logs once at the call site.
    pub fn from_path(path: PathBuf) -> Option<Self> {
        let cfg = Configuration::load(&path).ok()?;
        let name = cfg.name.clone().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| "(unnamed)".into())
        });
        Some(Self {
            path,
            name,
            description: cfg.description,
        })
    }
}

/// Scan `dir` for `*.toml` files, deserialize each, and return the
/// valid ones sorted by case-insensitive name. Missing dir is treated
/// as empty; broken files are skipped with a warning so one bad config
/// doesn't blow up the picker.
pub fn scan(dir: &Path) -> Vec<ConfigEntry> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            tracing::info!(
                target: "juballer::carla::picker",
                "scan {} skipped: {e}",
                dir.display()
            );
            return Vec::new();
        }
    };
    let mut entries: Vec<ConfigEntry> = read
        .filter_map(|r| r.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
        .filter_map(|p| match ConfigEntry::from_path(p.clone()) {
            Some(e) => Some(e),
            None => {
                tracing::warn!(
                    target: "juballer::carla::picker",
                    "skipping malformed carla config: {}",
                    p.display()
                );
                None
            }
        })
        .collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

pub struct PickerState {
    pages: Paginator<ConfigEntry>,
}

impl PickerState {
    pub fn new(entries: Vec<ConfigEntry>) -> Self {
        Self {
            pages: Paginator::new(entries, TILES_PER_PAGE),
        }
    }

    pub fn current_entries(&self) -> &[ConfigEntry] {
        self.pages.current_items()
    }

    pub fn entry_at_cell(&self, row: u8, col: u8) -> Option<&ConfigEntry> {
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

/// What the caller should do in response to a press while the picker
/// is open. Page nav variants are still returned (rather than mutating
/// the picker directly) so all dispatch happens in one place.
#[derive(Debug, Clone, PartialEq)]
pub enum PickerAction {
    /// Activate the configuration at this path — caller tears down
    /// the current `CarlaState` and rebuilds it from the file.
    Activate(PathBuf),
    /// Close the picker without changing the active configuration.
    Back,
    /// Exit carla mode entirely.
    Exit,
    PagePrev,
    PageNext,
    /// Press did not map to anything actionable (empty cell, off-page
    /// coordinates).
    None,
}

/// Pure event router: classify a `(row, col)` press against the
/// picker's current page. The caller handles state mutation /
/// re-loading; everything testable lives here.
pub fn classify_press(state: &PickerState, row: u8, col: u8) -> PickerAction {
    if row == NAV_ROW {
        return match col {
            c if c == NAV_PREV_COL => PickerAction::PagePrev,
            c if c == NAV_NEXT_COL => PickerAction::PageNext,
            c if c == NAV_BACK_COL => PickerAction::Back,
            c if c == NAV_EXIT_COL => PickerAction::Exit,
            _ => PickerAction::None,
        };
    }
    match state.entry_at_cell(row, col) {
        Some(entry) => PickerAction::Activate(entry.path.clone()),
        None => PickerAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(format!("{name}.toml"));
        std::fs::write(&path, body).unwrap();
        path
    }

    fn minimal_cfg(name: &str) -> String {
        format!(
            r#"
            name = "{name}"
            [[page]]
            "#
        )
    }

    #[test]
    fn scan_returns_empty_when_dir_missing() {
        let dir = std::env::temp_dir().join("juballer-carla-picker-missing-test-1234");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(scan(&dir).is_empty());
    }

    #[test]
    fn scan_returns_valid_configs_sorted_by_name_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "zeta", &minimal_cfg("Zeta Drum FX"));
        write_config(dir.path(), "alpha", &minimal_cfg("alpha cab"));
        write_config(dir.path(), "beta", &minimal_cfg("Beta EQ"));
        let entries = scan(dir.path());
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "alpha cab");
        assert_eq!(entries[1].name, "Beta EQ");
        assert_eq!(entries[2].name, "Zeta Drum FX");
    }

    #[test]
    fn scan_skips_files_that_fail_to_parse_without_blowing_up() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "ok", &minimal_cfg("OK"));
        write_config(dir.path(), "broken", "not = valid = toml");
        let entries = scan(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "OK");
    }

    #[test]
    fn config_entry_falls_back_to_file_stem_when_name_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), "no_name_field", "[[page]]");
        let entry = ConfigEntry::from_path(path).unwrap();
        assert_eq!(entry.name, "no_name_field");
    }

    fn fixture_picker(n: usize) -> PickerState {
        let entries: Vec<ConfigEntry> = (0..n)
            .map(|i| ConfigEntry {
                path: PathBuf::from(format!("/tmp/cfg{i}.toml")),
                name: format!("cfg-{i}"),
                description: None,
            })
            .collect();
        PickerState::new(entries)
    }

    #[test]
    fn picker_state_paginates_after_twelve_entries() {
        let p = fixture_picker(15);
        assert_eq!(p.page_count(), 2);
        assert_eq!(p.current_entries().len(), 12);
        assert_eq!(p.total(), 15);
    }

    #[test]
    fn entry_at_cell_returns_active_page_entry_for_in_range_coords() {
        let p = fixture_picker(8);
        let entry = p.entry_at_cell(0, 0).unwrap();
        assert_eq!(entry.name, "cfg-0");
        // Last visible cell at (2, 3) → index 11 → not present (only 8 entries)
        assert!(p.entry_at_cell(2, 3).is_none());
    }

    #[test]
    fn entry_at_cell_returns_none_for_nav_row() {
        let p = fixture_picker(12);
        for col in 0..4 {
            assert!(p.entry_at_cell(NAV_ROW, col).is_none());
        }
    }

    #[test]
    fn classify_press_routes_nav_row_to_action_variants() {
        let p = fixture_picker(2);
        assert_eq!(
            classify_press(&p, NAV_ROW, NAV_PREV_COL),
            PickerAction::PagePrev
        );
        assert_eq!(
            classify_press(&p, NAV_ROW, NAV_NEXT_COL),
            PickerAction::PageNext
        );
        assert_eq!(
            classify_press(&p, NAV_ROW, NAV_BACK_COL),
            PickerAction::Back
        );
        assert_eq!(
            classify_press(&p, NAV_ROW, NAV_EXIT_COL),
            PickerAction::Exit
        );
    }

    #[test]
    fn classify_press_returns_activate_for_in_range_tile() {
        let p = fixture_picker(2);
        let action = classify_press(&p, 0, 1);
        assert_eq!(
            action,
            PickerAction::Activate(PathBuf::from("/tmp/cfg1.toml"))
        );
    }

    #[test]
    fn classify_press_returns_none_for_empty_tile() {
        let p = fixture_picker(2);
        // (0,0) and (0,1) are populated; (1,0) is empty (only 2 entries).
        assert_eq!(classify_press(&p, 1, 0), PickerAction::None);
    }

    #[test]
    fn nav_constants_match_documented_layout() {
        assert_eq!(NAV_ROW, 3);
        assert_eq!(NAV_PREV_COL, 0);
        assert_eq!(NAV_NEXT_COL, 1);
        assert_eq!(NAV_BACK_COL, 2);
        assert_eq!(NAV_EXIT_COL, 3);
    }
}
