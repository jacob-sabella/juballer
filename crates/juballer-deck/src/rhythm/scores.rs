//! Persistent per-chart high-score book.
//!
//! Each [`ScoreRecord`] captures the score summary for one play. [`ScoreBook`]
//! groups records by `(chart_path, difficulty)` and is persisted as a single
//! JSON file under the deck config dir (default
//! `$XDG_CONFIG_HOME/juballer/deck/scores.json`).
//!
//! Notable invariants:
//!
//!   * Keys are canonicalised chart paths when possible, falling back to the
//!     original path on canonicalisation failure (e.g. chart has been moved
//!     since the play). Keeps entries stable across cwd changes.
//!   * Each key keeps its top [`TOP_N`] records, sorted by score descending.
//!   * The file is written atomically via [`crate::config::atomic_write`] so a
//!     crash mid-save won't corrupt the book.
//!   * Unknown keys / missing files are treated as an empty book rather than an
//!     error.
//!
//! UI-agnostic; the HUD and picker read the top record for a key and format
//! it themselves.

use super::judge::Grade;
use super::state::GameState;
use crate::config::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many records to retain per (chart, difficulty) key.
pub const TOP_N: usize = 10;

/// One session's score summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreRecord {
    pub score: u64,
    pub max_combo: u32,
    /// Stored as-is (may be negative-less percentage 0..100). `None`-valued
    /// accuracies are flattened to 0.0 on write; readers should treat
    /// zero-score as "not really played".
    pub accuracy_pct: f64,
    /// Grade histogram (Perfect / Great / Good / Poor / Miss → count). Optional
    /// grades with zero count may be omitted.
    pub grade_counts: HashMap<Grade, u32>,
    /// RFC3339 UTC timestamp of when the session ended. Stored as a string so
    /// we don't bind the on-disk format to chrono internals.
    pub played_at: String,
}

impl ScoreRecord {
    /// Build a record from a finished [`GameState`]. Timestamp is taken from
    /// the system clock at call time (UTC, RFC3339).
    pub fn from_state(state: &GameState) -> Self {
        let accuracy_pct = state.accuracy_pct().unwrap_or(0.0);
        let played_at = chrono::Utc::now().to_rfc3339();
        Self {
            score: state.score,
            max_combo: state.max_combo,
            accuracy_pct,
            grade_counts: state.grade_counts.clone(),
            played_at,
        }
    }
}

/// Composite key for a chart + difficulty pair. Kept as an opaque string
/// (`"{canonical_path}::{difficulty}"`) so it serialises cleanly as a JSON
/// object key.
fn make_key(chart_path: &Path, difficulty: &str) -> String {
    let canon = chart_path
        .canonicalize()
        .unwrap_or_else(|_| chart_path.to_path_buf());
    format!("{}::{}", canon.display(), difficulty)
}

/// In-memory book of per-chart records. Persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreBook {
    /// Map of composite key → ordered list of records (best first).
    #[serde(default)]
    entries: HashMap<String, Vec<ScoreRecord>>,
}

impl ScoreBook {
    /// Construct an empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Default on-disk path. Nested under the deck config dir.
    pub fn default_path() -> PathBuf {
        crate::config::paths::default_config_dir().join("scores.json")
    }

    /// Load from the given path. Missing file → empty book (Ok).
    /// Corrupt file → error.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(e) => return Err(e),
        };
        serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Load from the default path; equivalent to `load(&default_path())`.
    pub fn load_default() -> std::io::Result<Self> {
        Self::load(&Self::default_path())
    }

    /// Atomically write to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        atomic_write(path, &json)
    }

    /// Atomically write to the default path.
    pub fn save_default(&self) -> std::io::Result<()> {
        self.save(&Self::default_path())
    }

    /// Insert `record` under `(chart_path, difficulty)`, keeping the list
    /// sorted by score desc and capped at [`TOP_N`].
    pub fn record(&mut self, chart_path: &Path, difficulty: &str, record: ScoreRecord) {
        let key = make_key(chart_path, difficulty);
        let list = self.entries.entry(key).or_default();
        list.push(record);
        list.sort_by_key(|e| std::cmp::Reverse(e.score));
        if list.len() > TOP_N {
            list.truncate(TOP_N);
        }
    }

    /// Top-N records for this key, best first. Returns an empty slice if the
    /// chart has never been played.
    pub fn top_n(&self, chart_path: &Path, difficulty: &str) -> &[ScoreRecord] {
        let key = make_key(chart_path, difficulty);
        self.entries.get(&key).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Convenience: single best score (if any) for this key.
    pub fn best(&self, chart_path: &Path, difficulty: &str) -> Option<&ScoreRecord> {
        self.top_n(chart_path, difficulty).first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(score: u64) -> ScoreRecord {
        ScoreRecord {
            score,
            max_combo: 0,
            accuracy_pct: 0.0,
            grade_counts: HashMap::new(),
            played_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn record_inserts_a_new_entry() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a dummy chart file so canonicalize doesn't surprise us
        // (though we don't actually require it to exist for `record`).
        let chart = tmp.path().join("song.memon");
        std::fs::write(&chart, "{}").unwrap();

        let mut book = ScoreBook::new();
        assert!(book.best(&chart, "BSC").is_none());
        book.record(&chart, "BSC", rec(1234));
        let top = book.top_n(&chart, "BSC");
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].score, 1234);
        // Different difficulty is a separate bucket.
        assert!(book.best(&chart, "ADV").is_none());
    }

    #[test]
    fn top_n_sorts_desc_and_caps_at_ten() {
        let tmp = tempfile::tempdir().unwrap();
        let chart = tmp.path().join("song.memon");
        std::fs::write(&chart, "{}").unwrap();

        let mut book = ScoreBook::new();
        // Insert 15 records out of order.
        for s in [5u64, 12, 3, 20, 7, 15, 1, 30, 8, 14, 2, 11, 25, 9, 6] {
            book.record(&chart, "BSC", rec(s));
        }
        let top = book.top_n(&chart, "BSC");
        assert_eq!(top.len(), TOP_N);
        // Should be sorted descending.
        for w in top.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
        // Sorted desc: 30, 25, 20, 15, 14, 12, 11, 9, 8, 7.
        assert_eq!(top[0].score, 30);
        assert_eq!(top[TOP_N - 1].score, 7);
        // Values below 7 (5, 3, 1, 2, 6) should have been dropped.
        assert!(top.iter().all(|r| r.score >= 7));
    }

    #[test]
    fn save_then_load_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let chart = tmp.path().join("song.memon");
        std::fs::write(&chart, "{}").unwrap();

        let mut book = ScoreBook::new();
        let mut r = rec(500);
        r.max_combo = 12;
        r.accuracy_pct = 87.5;
        r.grade_counts.insert(Grade::Perfect, 3);
        r.grade_counts.insert(Grade::Miss, 1);
        book.record(&chart, "ADV", r.clone());
        book.record(&chart, "ADV", rec(100));
        book.record(&chart, "BSC", rec(999));

        let path = tmp.path().join("scores.json");
        book.save(&path).unwrap();
        let loaded = ScoreBook::load(&path).unwrap();

        // Same content for both keys.
        assert_eq!(
            loaded.top_n(&chart, "ADV").len(),
            book.top_n(&chart, "ADV").len()
        );
        assert_eq!(loaded.best(&chart, "ADV").unwrap(), &r);
        assert_eq!(loaded.best(&chart, "BSC").unwrap().score, 999);
    }

    #[test]
    fn load_missing_file_is_empty_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nope.json");
        let book = ScoreBook::load(&path).unwrap();
        assert!(book.entries.is_empty());
    }
}
