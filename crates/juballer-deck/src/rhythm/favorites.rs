//! Per-chart favorite flag, persisted as JSON.
//!
//! Mirrors the `scores.rs` / `chart_overrides.rs` storage pattern: one
//! JSON file at `<config>/favorites.json`, keys are canonicalised chart
//! paths, values are `true`. Missing file → empty book (no error).
//! Atomic write so a crash mid-toggle won't corrupt.
//!
//! Toggled from the picker by long-holding the NEXT cell on the focused
//! chart. Read by the filter pipeline when "Favorites only" is on.

use crate::config::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FavoriteBook {
    /// Canonicalised chart paths.
    paths: HashSet<String>,
}

impl FavoriteBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn default_path() -> PathBuf {
        crate::config::paths::default_config_dir().join("favorites.json")
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

    fn key(p: &Path) -> String {
        p.canonicalize()
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    pub fn is_favorite(&self, p: &Path) -> bool {
        self.paths.contains(&Self::key(p))
    }

    /// Returns the new state (true = now favorited).
    pub fn toggle(&mut self, p: &Path) -> bool {
        let k = Self::key(p);
        if self.paths.remove(&k) {
            false
        } else {
            self.paths.insert(k);
            true
        }
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_toggle() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("favs.json");
        let chart = tmp.path().join("song.memon");
        std::fs::write(&chart, "{}").unwrap();

        let mut book = FavoriteBook::new();
        assert!(!book.is_favorite(&chart));
        assert!(book.toggle(&chart));
        assert!(book.is_favorite(&chart));
        book.save(&p).unwrap();

        let loaded = FavoriteBook::load(&p).unwrap();
        assert!(loaded.is_favorite(&chart));

        let mut book2 = loaded;
        assert!(!book2.toggle(&chart));
        assert!(!book2.is_favorite(&chart));
    }

    #[test]
    fn missing_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("does-not-exist.json");
        let book = FavoriteBook::load(&p).unwrap();
        assert!(book.is_empty());
    }
}
