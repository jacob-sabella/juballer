//! Per-chart settings overrides — keyed by chart path.
//!
//! Carries an `audio_offset_ms` field per chart, set from the results
//! screen ("APPLY OFFSET TO THIS SONG"). The override takes precedence
//! over the global `[rhythm] audio_offset_ms` for that one chart, so a
//! player who needs different sync per song (e.g. an ogg with baked-in
//! DAW latency) can dial it in once and have it stick.
//!
//! Storage mirrors the score book: JSON file under the deck config dir,
//! atomic write, missing = empty (no error). Keys are canonicalised
//! chart paths, falling back to the original on canonicalisation
//! failure (chart moved since last play).

use crate::config::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChartOverride {
    /// Per-chart audio offset in milliseconds. Replaces the global
    /// `[rhythm] audio_offset_ms` for this chart only.
    pub audio_offset_ms: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChartOverrideBook {
    /// chart path (string-encoded) → override
    pub by_chart: HashMap<String, ChartOverride>,
}

impl ChartOverrideBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn default_path() -> PathBuf {
        crate::config::paths::default_config_dir().join("chart_overrides.json")
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
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

    fn key(chart_path: &Path) -> String {
        chart_path
            .canonicalize()
            .unwrap_or_else(|_| chart_path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    pub fn get(&self, chart_path: &Path) -> Option<&ChartOverride> {
        self.by_chart.get(&Self::key(chart_path))
    }

    pub fn set_offset(&mut self, chart_path: &Path, offset_ms: i32) {
        self.by_chart
            .entry(Self::key(chart_path))
            .or_default()
            .audio_offset_ms = offset_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("ovr.json");
        let chart = tmp.path().join("song.memon");
        std::fs::write(&chart, "{}").unwrap();

        let mut book = ChartOverrideBook::new();
        book.set_offset(&chart, -28);
        book.save(&p).unwrap();

        let loaded = ChartOverrideBook::load(&p).unwrap();
        let ovr = loaded.get(&chart).expect("chart override stored");
        assert_eq!(ovr.audio_offset_ms, -28);
    }

    #[test]
    fn missing_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("does-not-exist.json");
        let book = ChartOverrideBook::load(&p).unwrap();
        assert!(book.by_chart.is_empty());
    }

    #[test]
    fn set_offset_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let chart = tmp.path().join("song.memon");
        std::fs::write(&chart, "{}").unwrap();
        let mut book = ChartOverrideBook::new();
        book.set_offset(&chart, 10);
        book.set_offset(&chart, -42);
        assert_eq!(book.get(&chart).unwrap().audio_offset_ms, -42);
    }
}
