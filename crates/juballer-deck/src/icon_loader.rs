//! Load + cache tile icons from disk. Keyed by absolute path to avoid double-reading.

use crate::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct IconLoader {
    assets_root: PathBuf,
    cache: HashMap<PathBuf, Vec<u8>>,
}

impl IconLoader {
    pub fn new(assets_root: PathBuf) -> Self {
        Self {
            assets_root,
            cache: HashMap::new(),
        }
    }

    pub fn load(&mut self, path: &Path) -> Result<&[u8]> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.assets_root.join(path)
        };
        if !self.cache.contains_key(&abs) {
            let bytes = std::fs::read(&abs)?;
            self.cache.insert(abs.clone(), bytes);
        }
        Ok(self.cache.get(&abs).unwrap())
    }

    pub fn assets_root(&self) -> &Path {
        &self.assets_root
    }
}
