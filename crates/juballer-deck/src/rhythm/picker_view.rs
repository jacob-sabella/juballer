//! Sort + filter state for the chart picker, plus the pure-function
//! pipeline that turns a flat library list into a sorted/filtered view
//! the paginator displays.
//!
//! State layout: a single [`PickerView`] struct, persisted to JSON at
//! `<config>/picker_view.json`. Survives picker exits so the user keeps
//! their current sort/filter on next launch.
//!
//! Pipeline:
//!   1. `apply_filters(entries, view, &favs)` keeps only entries that
//!      match every active filter (pack, favorites-only, level range,
//!      diff-must-exist).
//!   2. `apply_sort(entries, view)` orders the surviving list per the
//!      active [`SortMode`] + [`SortDirection`].
//!
//! Both steps are pure functions over `&[ChartEntry]` so the picker can
//! call them every time the view changes without juggling indices.

use super::favorites::FavoriteBook;
use super::picker::ChartEntry;
use crate::config::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    /// Library order (whatever scan() returns).
    Default,
    Title,
    Artist,
    Bpm,
    Level,
    Notes,
}

impl SortMode {
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Default => "default",
            SortMode::Title => "title",
            SortMode::Artist => "artist",
            SortMode::Bpm => "bpm",
            SortMode::Level => "level",
            SortMode::Notes => "notes",
        }
    }

    pub const ALL: [SortMode; 6] = [
        SortMode::Default,
        SortMode::Title,
        SortMode::Artist,
        SortMode::Bpm,
        SortMode::Level,
        SortMode::Notes,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn label(self) -> &'static str {
        match self {
            SortDirection::Asc => "↑ asc",
            SortDirection::Desc => "↓ desc",
        }
    }
    pub fn flip(self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FavoriteFilter {
    All,
    OnlyFavs,
}

impl FavoriteFilter {
    pub fn label(self) -> &'static str {
        match self {
            FavoriteFilter::All => "all",
            FavoriteFilter::OnlyFavs => "only ★",
        }
    }
    pub fn next(self) -> Self {
        match self {
            FavoriteFilter::All => FavoriteFilter::OnlyFavs,
            FavoriteFilter::OnlyFavs => FavoriteFilter::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DifficultyFilter {
    Any,
    Bsc,
    Adv,
    Ext,
}

impl DifficultyFilter {
    pub fn label(self) -> &'static str {
        match self {
            DifficultyFilter::Any => "any",
            DifficultyFilter::Bsc => "BSC",
            DifficultyFilter::Adv => "ADV",
            DifficultyFilter::Ext => "EXT",
        }
    }
    pub const ALL: [DifficultyFilter; 4] = [
        DifficultyFilter::Any,
        DifficultyFilter::Bsc,
        DifficultyFilter::Adv,
        DifficultyFilter::Ext,
    ];
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|d| *d == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
    fn matches(self, e: &ChartEntry) -> bool {
        let want = match self {
            DifficultyFilter::Any => return true,
            DifficultyFilter::Bsc => "BSC",
            DifficultyFilter::Adv => "ADV",
            DifficultyFilter::Ext => "EXT",
        };
        e.difficulties.iter().any(|d| d == want)
    }
}

/// "Pack" = the immediate parent directory of the chart's containing
/// folder. With a layout of
/// `~/.config/juballer/rhythm/charts/<pack>/<song>/song.memon` this
/// surfaces the `<pack>` directory's name.
fn pack_of(e: &ChartEntry) -> Option<String> {
    let parent = e.path.parent()?; // .../<pack>/<song>/
    let pack = parent.parent()?; // .../<pack>/
    pack.file_name()?.to_str().map(str::to_owned)
}

/// Filter cycling through the set of packs found in the library plus an
/// "all" sentinel. Stored as a string so it survives library churn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackFilter {
    /// None = all packs, Some(name) = only that pack
    pub only: Option<String>,
}

impl PackFilter {
    pub fn label(&self) -> String {
        self.only.clone().unwrap_or_else(|| "all packs".to_string())
    }
    pub fn next(&self, all_packs: &[String]) -> Self {
        if all_packs.is_empty() {
            return Self { only: None };
        }
        let mut chain: Vec<Option<String>> = vec![None];
        chain.extend(all_packs.iter().cloned().map(Some));
        let idx = chain.iter().position(|p| p == &self.only).unwrap_or(0);
        Self {
            only: chain[(idx + 1) % chain.len()].clone(),
        }
    }
    fn matches(&self, e: &ChartEntry) -> bool {
        match &self.only {
            None => true,
            Some(want) => pack_of(e).as_deref() == Some(want.as_str()),
        }
    }
}

/// Top-level persisted view state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickerView {
    pub sort: SortMode,
    pub direction: SortDirection,
    pub favorite_filter: FavoriteFilter,
    pub difficulty_filter: DifficultyFilter,
    pub pack_filter: PackFilter,
}

impl Default for PickerView {
    fn default() -> Self {
        Self {
            sort: SortMode::Default,
            direction: SortDirection::Asc,
            favorite_filter: FavoriteFilter::All,
            difficulty_filter: DifficultyFilter::Any,
            pack_filter: PackFilter::default(),
        }
    }
}

impl PickerView {
    pub fn default_path() -> PathBuf {
        crate::config::paths::default_config_dir().join("picker_view.json")
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn load_default() -> std::io::Result<Self> {
        Self::load(&Self::default_path())
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        atomic_write(path, &json)
    }

    pub fn save_default(&self) -> std::io::Result<()> {
        self.save(&Self::default_path())
    }
}

/// All distinct pack names in `entries`, sorted alphabetically. Used by
/// the filter UI to cycle through pack options.
pub fn discover_packs(entries: &[ChartEntry]) -> Vec<String> {
    let mut s = std::collections::BTreeSet::new();
    for e in entries {
        if let Some(p) = pack_of(e) {
            s.insert(p);
        }
    }
    s.into_iter().collect()
}

/// Apply all active filters. Returns a fresh `Vec` of clones so the
/// caller can drop it into a new `Paginator`.
pub fn apply_filters(
    entries: &[ChartEntry],
    view: &PickerView,
    favs: &FavoriteBook,
) -> Vec<ChartEntry> {
    entries
        .iter()
        .filter(|e| {
            view.pack_filter.matches(e)
                && view.difficulty_filter.matches(e)
                && match view.favorite_filter {
                    FavoriteFilter::All => true,
                    FavoriteFilter::OnlyFavs => favs.is_favorite(&e.path),
                }
        })
        .cloned()
        .collect()
}

/// Sort in-place per [`SortMode`] + [`SortDirection`]. Title / artist
/// use locale-insensitive case-folded comparison so "abc" and "ABC"
/// don't sort apart. BPM falls back to title for ties so the order is
/// stable across runs.
pub fn apply_sort(entries: &mut [ChartEntry], view: &PickerView) {
    let cmp_title =
        |a: &ChartEntry, b: &ChartEntry| a.title.to_lowercase().cmp(&b.title.to_lowercase());
    match view.sort {
        SortMode::Default => {} // leave caller's order
        SortMode::Title => entries.sort_by(cmp_title),
        SortMode::Artist => entries.sort_by(|a, b| {
            a.artist
                .to_lowercase()
                .cmp(&b.artist.to_lowercase())
                .then_with(|| cmp_title(a, b))
        }),
        SortMode::Bpm => entries.sort_by(|a, b| {
            a.bpm
                .partial_cmp(&b.bpm)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| cmp_title(a, b))
        }),
        // Level sorts by the numeric encoded difficulty index — for
        // multi-diff charts uses the EXT-equivalent (last) entry as a
        // proxy for "how hard is this chart at peak".
        SortMode::Level => entries.sort_by(|a, b| {
            let av = a.difficulties.len();
            let bv = b.difficulties.len();
            av.cmp(&bv).then_with(|| cmp_title(a, b))
        }),
        SortMode::Notes => entries.sort_by(|a, b| {
            a.note_count
                .cmp(&b.note_count)
                .then_with(|| cmp_title(a, b))
        }),
    }
    if matches!(view.direction, SortDirection::Desc) && !matches!(view.sort, SortMode::Default) {
        entries.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhythm::chart::Preview;
    use std::path::PathBuf;

    fn entry(title: &str, artist: &str, bpm: f64, notes: usize, diffs: &[&str]) -> ChartEntry {
        ChartEntry {
            path: PathBuf::from(format!("/charts/pack_a/{title}/song.memon")),
            title: title.into(),
            artist: artist.into(),
            difficulties: diffs.iter().map(|s| s.to_string()).collect(),
            bpm,
            note_count: notes,
            audio_path: PathBuf::new(),
            jacket_path: None,
            mini_path: None,
            banner_path: None,
            preview: None::<Preview>,
        }
    }

    #[test]
    fn sort_title_asc_desc_round_trip() {
        let mut v = vec![
            entry("Charlie", "x", 120.0, 100, &["BSC"]),
            entry("alpha", "x", 110.0, 200, &["BSC"]),
            entry("Bravo", "x", 130.0, 300, &["BSC"]),
        ];
        let mut view = PickerView {
            sort: SortMode::Title,
            ..PickerView::default()
        };
        apply_sort(&mut v, &view);
        assert_eq!(
            v.iter().map(|e| e.title.clone()).collect::<Vec<_>>(),
            vec!["alpha", "Bravo", "Charlie"]
        );
        view.direction = SortDirection::Desc;
        apply_sort(&mut v, &view);
        assert_eq!(
            v.iter().map(|e| e.title.clone()).collect::<Vec<_>>(),
            vec!["Charlie", "Bravo", "alpha"]
        );
    }

    #[test]
    fn favorites_filter_keeps_only_favored() {
        let v = vec![
            entry("song1", "x", 120.0, 100, &["BSC"]),
            entry("song2", "x", 120.0, 100, &["BSC"]),
        ];
        let mut favs = FavoriteBook::new();
        favs.toggle(&v[1].path);
        let view = PickerView {
            favorite_filter: FavoriteFilter::OnlyFavs,
            ..PickerView::default()
        };
        let kept = apply_filters(&v, &view, &favs);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].title, "song2");
    }

    #[test]
    fn difficulty_filter_drops_charts_lacking_diff() {
        let v = vec![
            entry("a", "x", 120.0, 100, &["BSC", "ADV"]),
            entry("b", "x", 120.0, 100, &["BSC"]),
            entry("c", "x", 120.0, 100, &["EXT"]),
        ];
        let view = PickerView {
            difficulty_filter: DifficultyFilter::Adv,
            ..PickerView::default()
        };
        let kept = apply_filters(&v, &view, &FavoriteBook::new());
        assert_eq!(
            kept.iter().map(|e| e.title.clone()).collect::<Vec<_>>(),
            vec!["a"]
        );
    }

    #[test]
    fn discover_packs_returns_unique_sorted() {
        let mut v = vec![
            entry("a", "x", 120.0, 100, &["BSC"]),
            entry("b", "x", 120.0, 100, &["BSC"]),
        ];
        v[1].path = PathBuf::from("/charts/pack_b/b/song.memon");
        let packs = discover_packs(&v);
        assert_eq!(packs, vec!["pack_a", "pack_b"]);
    }
}
