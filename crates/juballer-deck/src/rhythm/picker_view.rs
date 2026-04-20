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
use super::scores::ScoreBook;
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
    /// Most recently played chart first by default. Charts that have
    /// never been played sort to the bottom in either direction.
    LastPlayed,
    /// Personal best score for the active difficulty. Higher score
    /// first by default. Unscored charts sort to the bottom.
    Score,
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
            SortMode::LastPlayed => "last played",
            SortMode::Score => "score",
        }
    }

    pub const ALL: [SortMode; 8] = [
        SortMode::Default,
        SortMode::Title,
        SortMode::Artist,
        SortMode::Bpm,
        SortMode::Level,
        SortMode::Notes,
        SortMode::LastPlayed,
        SortMode::Score,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Default direction for this mode. Most users want last-played
    /// and score in *descending* order (most recent / highest first);
    /// the alphabetic / numeric modes default to ascending.
    pub fn default_direction(self) -> SortDirection {
        match self {
            SortMode::LastPlayed | SortMode::Score => SortDirection::Desc,
            _ => SortDirection::Asc,
        }
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
///
/// `scores` + `difficulty` are consulted for the [`SortMode::LastPlayed`]
/// and [`SortMode::Score`] modes; other modes ignore them. Charts with
/// no entry in the score book sort to the bottom in either direction
/// (i.e. they never land at the top of a "last played" list just
/// because their absence happens to compare lower).
pub fn apply_sort(
    entries: &mut [ChartEntry],
    view: &PickerView,
    scores: &ScoreBook,
    difficulty: &str,
) {
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
        SortMode::LastPlayed => {
            // Sort ascending by timestamp (oldest → newest). The
            // direction-flip below puts most recent first when the
            // user picks Desc (the default for this mode). Unscored
            // charts get a sentinel that, after the optional reverse,
            // still lands them at the bottom of either direction.
            entries.sort_by(|a, b| {
                let av = scores.last_played(&a.path, difficulty);
                let bv = scores.last_played(&b.path, difficulty);
                last_played_cmp(av, bv, view.direction)
                    .then_with(|| cmp_title(a, b))
            });
        }
        SortMode::Score => {
            entries.sort_by(|a, b| {
                let av = scores.best(&a.path, difficulty).map(|r| r.score);
                let bv = scores.best(&b.path, difficulty).map(|r| r.score);
                score_cmp(av, bv, view.direction).then_with(|| cmp_title(a, b))
            });
        }
    }
    // The two score-aware modes do their own direction handling above
    // so the unscored-charts-to-the-bottom invariant survives the
    // flip; everything else flips here.
    let already_directional = matches!(view.sort, SortMode::LastPlayed | SortMode::Score);
    if matches!(view.direction, SortDirection::Desc)
        && !matches!(view.sort, SortMode::Default)
        && !already_directional
    {
        entries.reverse();
    }
}

/// Compare two timestamp slots so unscored entries always sort to the
/// bottom regardless of direction. In ASC, earlier dates come first
/// (None at end); in DESC, later dates come first (None still at end).
fn last_played_cmp(a: Option<&str>, b: Option<&str>, dir: SortDirection) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
        (Some(a), Some(b)) => match dir {
            SortDirection::Asc => a.cmp(b),
            SortDirection::Desc => b.cmp(a),
        },
    }
}

/// Same idea for scores: Some(score) always beats None, then within
/// the Some/Some case the direction picks ascending or descending.
fn score_cmp(a: Option<u64>, b: Option<u64>, dir: SortDirection) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
        (Some(a), Some(b)) => match dir {
            SortDirection::Asc => a.cmp(&b),
            SortDirection::Desc => b.cmp(&a),
        },
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
        apply_sort(&mut v, &view, &ScoreBook::new(), "BSC");
        assert_eq!(
            v.iter().map(|e| e.title.clone()).collect::<Vec<_>>(),
            vec!["alpha", "Bravo", "Charlie"]
        );
        view.direction = SortDirection::Desc;
        apply_sort(&mut v, &view, &ScoreBook::new(), "BSC");
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

    fn record_with_score_at(score: u64, played_at: &str) -> super::super::scores::ScoreRecord {
        super::super::scores::ScoreRecord {
            score,
            max_combo: 0,
            accuracy_pct: 0.0,
            grade_counts: std::collections::HashMap::new(),
            played_at: played_at.into(),
        }
    }

    fn populated_book(rows: &[(&str, u64, &str)]) -> ScoreBook {
        let mut book = ScoreBook::new();
        for (title, score, ts) in rows {
            // Recompute the same canonical path the entry helper uses.
            let path = PathBuf::from(format!("/charts/pack_a/{title}/song.memon"));
            book.record(&path, "BSC", record_with_score_at(*score, ts));
        }
        book
    }

    #[test]
    fn sort_score_desc_puts_highest_first_unscored_last() {
        let mut v = vec![
            entry("alpha", "x", 120.0, 100, &["BSC"]),
            entry("Bravo", "x", 120.0, 100, &["BSC"]),
            entry("Charlie", "x", 120.0, 100, &["BSC"]),
        ];
        let book = populated_book(&[
            ("alpha", 5_000, "2026-04-19T18:00:00Z"),
            ("Bravo", 9_000, "2026-04-19T19:00:00Z"),
            // Charlie unscored intentionally.
        ]);
        let view = PickerView {
            sort: SortMode::Score,
            direction: SortDirection::Desc,
            ..PickerView::default()
        };
        apply_sort(&mut v, &view, &book, "BSC");
        let titles: Vec<String> = v.iter().map(|e| e.title.clone()).collect();
        assert_eq!(titles, vec!["Bravo", "alpha", "Charlie"]);
    }

    #[test]
    fn sort_score_asc_puts_lowest_first_unscored_still_last() {
        let mut v = vec![
            entry("alpha", "x", 120.0, 100, &["BSC"]),
            entry("Bravo", "x", 120.0, 100, &["BSC"]),
            entry("Charlie", "x", 120.0, 100, &["BSC"]),
        ];
        let book = populated_book(&[
            ("alpha", 5_000, "2026-04-19T18:00:00Z"),
            ("Bravo", 9_000, "2026-04-19T19:00:00Z"),
        ]);
        let view = PickerView {
            sort: SortMode::Score,
            direction: SortDirection::Asc,
            ..PickerView::default()
        };
        apply_sort(&mut v, &view, &book, "BSC");
        let titles: Vec<String> = v.iter().map(|e| e.title.clone()).collect();
        // Lowest score first; unscored Charlie still pinned to the bottom.
        assert_eq!(titles, vec!["alpha", "Bravo", "Charlie"]);
    }

    #[test]
    fn sort_last_played_desc_orders_by_most_recent_session() {
        let mut v = vec![
            entry("alpha", "x", 120.0, 100, &["BSC"]),
            entry("Bravo", "x", 120.0, 100, &["BSC"]),
            entry("Charlie", "x", 120.0, 100, &["BSC"]),
        ];
        let book = populated_book(&[
            ("alpha", 1, "2026-04-19T18:00:00Z"),
            ("Bravo", 1, "2026-04-19T19:30:00Z"),
            ("Charlie", 1, "2026-04-19T17:00:00Z"),
        ]);
        let view = PickerView {
            sort: SortMode::LastPlayed,
            direction: SortDirection::Desc,
            ..PickerView::default()
        };
        apply_sort(&mut v, &view, &book, "BSC");
        let titles: Vec<String> = v.iter().map(|e| e.title.clone()).collect();
        assert_eq!(titles, vec!["Bravo", "alpha", "Charlie"]);
    }

    #[test]
    fn sort_last_played_pins_never_played_charts_to_bottom_in_asc_too() {
        let mut v = vec![
            entry("alpha", "x", 120.0, 100, &["BSC"]),
            entry("Bravo", "x", 120.0, 100, &["BSC"]),
            entry("Never", "x", 120.0, 100, &["BSC"]),
        ];
        let book = populated_book(&[
            ("alpha", 1, "2026-04-19T18:00:00Z"),
            ("Bravo", 1, "2026-04-19T19:00:00Z"),
        ]);
        let view = PickerView {
            sort: SortMode::LastPlayed,
            direction: SortDirection::Asc,
            ..PickerView::default()
        };
        apply_sort(&mut v, &view, &book, "BSC");
        let titles: Vec<String> = v.iter().map(|e| e.title.clone()).collect();
        // Asc: oldest first, Never always last.
        assert_eq!(titles, vec!["alpha", "Bravo", "Never"]);
    }

    #[test]
    fn sort_score_only_consults_active_difficulty_records() {
        let mut v = vec![
            entry("alpha", "x", 120.0, 100, &["BSC", "ADV"]),
            entry("Bravo", "x", 120.0, 100, &["BSC", "ADV"]),
        ];
        let mut book = ScoreBook::new();
        // alpha has a higher BSC than Bravo, but Bravo's higher score
        // is on ADV — sorting by BSC should keep alpha on top.
        book.record(
            &PathBuf::from("/charts/pack_a/alpha/song.memon"),
            "BSC",
            record_with_score_at(8_000, "2026-04-19T18:00:00Z"),
        );
        book.record(
            &PathBuf::from("/charts/pack_a/Bravo/song.memon"),
            "BSC",
            record_with_score_at(2_000, "2026-04-19T18:00:00Z"),
        );
        book.record(
            &PathBuf::from("/charts/pack_a/Bravo/song.memon"),
            "ADV",
            record_with_score_at(99_000, "2026-04-19T19:00:00Z"),
        );
        let view = PickerView {
            sort: SortMode::Score,
            direction: SortDirection::Desc,
            ..PickerView::default()
        };
        apply_sort(&mut v, &view, &book, "BSC");
        let titles: Vec<String> = v.iter().map(|e| e.title.clone()).collect();
        assert_eq!(titles, vec!["alpha", "Bravo"]);
    }

    #[test]
    fn last_played_returns_most_recent_record_across_replays() {
        let path = PathBuf::from("/charts/pack_a/alpha/song.memon");
        let mut book = ScoreBook::new();
        book.record(
            &path,
            "BSC",
            record_with_score_at(1, "2026-04-19T18:00:00Z"),
        );
        book.record(
            &path,
            "BSC",
            record_with_score_at(2, "2026-04-19T18:30:00Z"),
        );
        book.record(
            &path,
            "BSC",
            record_with_score_at(3, "2026-04-19T17:00:00Z"),
        );
        // RFC3339 strings sort lexicographically as time order, so the
        // 18:30 entry wins.
        let last = book.last_played(&path, "BSC").unwrap();
        assert_eq!(last, "2026-04-19T18:30:00Z");
    }

    #[test]
    fn sort_mode_default_direction_inverts_for_score_aware_modes() {
        assert_eq!(SortMode::Title.default_direction(), SortDirection::Asc);
        assert_eq!(SortMode::Score.default_direction(), SortDirection::Desc);
        assert_eq!(
            SortMode::LastPlayed.default_direction(),
            SortDirection::Desc
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
