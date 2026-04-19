//! Persisted deck state (counters, toggles, last page).
//!
//! Wraps `config::schema::StateFile` with an in-memory shadow + dirty flag, and writes
//! back to state.toml on shutdown / periodic flush.

use crate::config::schema::StateFile;
use crate::{Error, Result};
use indexmap::IndexMap;
use std::path::PathBuf;

pub struct StateStore {
    path: PathBuf,
    inner: StateFile,
    dirty: bool,
}

impl StateStore {
    pub fn open(path: PathBuf) -> Result<Self> {
        let inner = if path.exists() {
            let s = std::fs::read_to_string(&path)?;
            toml::from_str(&s).map_err(|source| Error::ConfigParse {
                path: path.clone(),
                source,
            })?
        } else {
            StateFile::default()
        };
        Ok(Self {
            path,
            inner,
            dirty: false,
        })
    }

    pub fn last_active_page(&self) -> Option<&str> {
        self.inner.last_active_page.as_deref()
    }

    pub fn set_last_active_page(&mut self, page: impl Into<String>) {
        self.inner.last_active_page = Some(page.into());
        self.dirty = true;
    }

    pub fn binding(&self, id: &str) -> Option<&serde_json::Value> {
        self.inner.bindings.get(id)
    }

    pub fn set_binding(&mut self, id: impl Into<String>, value: serde_json::Value) {
        self.inner.bindings.insert(id.into(), value);
        self.dirty = true;
    }

    pub fn bindings(&self) -> &IndexMap<String, serde_json::Value> {
        &self.inner.bindings
    }

    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let s = toml::to_string(&self.inner)
            .map_err(|e| Error::Config(format!("serialize state: {e}")))?;
        std::fs::write(&self.path, s)?;
        self.dirty = false;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_empty_then_write_roundtrip() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("state.toml");
        let mut s = StateStore::open(p.clone()).unwrap();
        s.set_last_active_page("home");
        s.set_binding("home:0,0", serde_json::json!({ "count": 3 }));
        s.flush().unwrap();

        let s2 = StateStore::open(p).unwrap();
        assert_eq!(s2.last_active_page(), Some("home"));
        assert_eq!(
            s2.binding("home:0,0"),
            Some(&serde_json::json!({ "count": 3 }))
        );
    }

    #[test]
    fn flush_noop_when_clean() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("state.toml");
        let mut s = StateStore::open(p.clone()).unwrap();
        // No sets, no flush writes.
        s.flush().unwrap();
        assert!(!p.exists());
    }
}
