//! Load the deck config tree from disk into a single in-memory snapshot.

use super::paths::DeckPaths;
use super::schema::{DeckConfig, PageConfig, ProfileMeta, StateFile};
use crate::{Error, Result};
use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct ConfigTree {
    pub deck: DeckConfig,
    pub profiles: IndexMap<String, ProfileTree>,
    pub state: StateFile,
    /// Pages shipped by plugins, keyed `<plugin_name>:<page_name>`. Loaded from
    /// `<plugins_dir>/<plugin>/pages/<name>.toml` at startup for every name listed in
    /// the plugin's manifest `pages` array. Resolved alongside profile pages.
    pub plugin_pages: IndexMap<String, PageConfig>,
}

#[derive(Debug, Clone)]
pub struct ProfileTree {
    pub meta: ProfileMeta,
    pub pages: IndexMap<String, PageConfig>,
}

impl ConfigTree {
    pub fn load(paths: &DeckPaths) -> Result<Self> {
        let deck = load_deck(&paths.deck_toml)?;
        let mut profiles = IndexMap::new();
        if paths.profiles_dir.exists() {
            for entry in std::fs::read_dir(&paths.profiles_dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry
                    .file_name()
                    .to_str()
                    .ok_or_else(|| {
                        Error::Config(format!("non-utf8 profile dir: {:?}", entry.file_name()))
                    })?
                    .to_string();
                profiles.insert(name.clone(), load_profile(paths, &name)?);
            }
        }
        let state = if paths.state_toml.exists() {
            let s = std::fs::read_to_string(&paths.state_toml)?;
            toml::from_str(&s).map_err(|source| Error::ConfigParse {
                path: paths.state_toml.clone(),
                source,
            })?
        } else {
            StateFile::default()
        };
        let plugin_pages = load_plugin_pages(&paths.plugins_dir);
        Ok(Self {
            deck,
            profiles,
            state,
            plugin_pages,
        })
    }

    /// Look up a page by the exact name stored in `active_page`. Accepts either a
    /// profile-local page name (looked up in the active profile's `pages` map) or a
    /// plugin-namespaced name like `discord:overview` (looked up in `plugin_pages`).
    pub fn lookup_page(&self, page_name: &str) -> Option<&PageConfig> {
        if page_name.contains(':') {
            if let Some(p) = self.plugin_pages.get(page_name) {
                return Some(p);
            }
        }
        self.active_profile()
            .ok()
            .and_then(|p| p.pages.get(page_name))
    }

    pub fn active_profile(&self) -> Result<&ProfileTree> {
        self.profiles.get(&self.deck.active_profile).ok_or_else(|| {
            Error::Config(format!(
                "active_profile '{}' not found",
                self.deck.active_profile
            ))
        })
    }
}

fn load_deck(path: &std::path::Path) -> Result<DeckConfig> {
    let s = std::fs::read_to_string(path)?;
    toml::from_str(&s).map_err(|source| Error::ConfigParse {
        path: path.to_path_buf(),
        source,
    })
}

fn load_plugin_pages(plugins_dir: &std::path::Path) -> IndexMap<String, PageConfig> {
    let mut out = IndexMap::new();
    if !plugins_dir.exists() {
        return out;
    }
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("plugin_pages: read_dir {:?}: {}", plugins_dir, e);
            return out;
        }
    };
    for entry in entries.flatten() {
        if entry.file_type().map(|t| !t.is_dir()).unwrap_or(true) {
            continue;
        }
        let dir = entry.path();
        let manifest_path = dir.join("manifest.toml");
        if !manifest_path.exists() {
            continue;
        }
        let manifest = match crate::plugin::manifest::PluginManifest::load(&manifest_path) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("plugin_pages: manifest {:?}: {}", manifest_path, e);
                continue;
            }
        };
        if manifest.pages.is_empty() {
            continue;
        }
        for page_name in &manifest.pages {
            let p = dir.join("pages").join(format!("{page_name}.toml"));
            if !p.exists() {
                tracing::warn!(
                    "plugin {}: declared page '{}' missing at {:?}",
                    manifest.name,
                    page_name,
                    p
                );
                continue;
            }
            let s = match std::fs::read_to_string(&p) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("plugin_pages: read {:?}: {}", p, e);
                    continue;
                }
            };
            let page: PageConfig = match toml::from_str(&s) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("plugin_pages: parse {:?}: {}", p, e);
                    continue;
                }
            };
            let key = format!("{}:{}", manifest.name, page_name);
            out.insert(key, page);
        }
    }
    out
}

fn load_profile(paths: &DeckPaths, name: &str) -> Result<ProfileTree> {
    let meta_path = paths.profile_meta_toml(name);
    let meta_str = std::fs::read_to_string(&meta_path)?;
    let meta: ProfileMeta = toml::from_str(&meta_str).map_err(|source| Error::ConfigParse {
        path: meta_path.clone(),
        source,
    })?;

    let mut pages = IndexMap::new();
    for page_name in &meta.pages {
        let p = paths.profile_page_toml(name, page_name);
        let s = std::fs::read_to_string(&p)?;
        let page: PageConfig = toml::from_str(&s).map_err(|source| Error::ConfigParse {
            path: p.clone(),
            source,
        })?;
        pages.insert(page_name.clone(), page);
    }
    Ok(ProfileTree { meta, pages })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &std::path::Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, DeckPaths) {
        let dir = tempdir().unwrap();
        let paths = DeckPaths::from_root(dir.path().to_path_buf());
        write(
            &paths.deck_toml,
            r##"
version = 1
active_profile = "homelab"

[editor]
bind = "127.0.0.1:7373"

[render]

[log]
level = "info"
"##,
        );
        write(
            &paths.profile_meta_toml("homelab"),
            r##"
name = "homelab"
default_page = "home"
pages = ["home"]

[env]
grafana_base = "http://grafana"
"##,
        );
        write(
            &paths.profile_page_toml("homelab", "home"),
            r##"
[meta]
title = "home"

[[button]]
row = 0
col = 0
action = "media.playpause"
"##,
        );
        (dir, paths)
    }

    #[test]
    fn loads_full_tree() {
        let (_dir, paths) = fixture();
        let tree = ConfigTree::load(&paths).unwrap();
        assert_eq!(tree.deck.active_profile, "homelab");
        let p = tree.active_profile().unwrap();
        assert_eq!(p.meta.default_page, "home");
        assert!(p.pages.contains_key("home"));
        assert_eq!(p.pages["home"].buttons.len(), 1);
    }

    #[test]
    fn missing_page_errors() {
        let (dir, paths) = fixture();
        std::fs::remove_file(paths.profile_page_toml("homelab", "home")).unwrap();
        let err = ConfigTree::load(&paths).unwrap_err();
        match err {
            Error::ConfigIo(_) => {}
            other => panic!("wrong error variant: {other:?}"),
        }
        drop(dir);
    }

    #[test]
    fn missing_active_profile_errors() {
        let (_dir, paths) = fixture();
        std::fs::remove_dir_all(paths.profile_dir("homelab")).unwrap();
        let tree = ConfigTree::load(&paths).unwrap();
        let err = tree.active_profile().unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong error variant: {other:?}"),
        }
    }

    #[test]
    fn state_optional() {
        let (_dir, paths) = fixture();
        let tree = ConfigTree::load(&paths).unwrap();
        assert!(tree.state.last_active_page.is_none());
    }

    #[test]
    fn plugin_pages_loaded_and_namespaced() {
        let (dir, paths) = fixture();
        let plugin_dir = paths.plugins_dir.join("discord");
        write(
            &plugin_dir.join("manifest.toml"),
            r#"
name = "discord"
version = "0.1.0"
entry_point = "main.py"
language = "python"
pages = ["overview"]
"#,
        );
        write(
            &plugin_dir.join("pages").join("overview.toml"),
            r#"
[meta]
title = "Discord Overview"
"#,
        );
        let tree = ConfigTree::load(&paths).unwrap();
        assert!(tree.plugin_pages.contains_key("discord:overview"));
        let p = tree.lookup_page("discord:overview").unwrap();
        assert_eq!(p.meta.title, "Discord Overview");
        // Profile pages still resolvable via the same helper.
        assert!(tree.lookup_page("home").is_some());
        drop(dir);
    }

    #[test]
    fn plugin_pages_skipped_when_file_missing() {
        let (dir, paths) = fixture();
        let plugin_dir = paths.plugins_dir.join("ghost");
        write(
            &plugin_dir.join("manifest.toml"),
            r#"
name = "ghost"
version = "0.1.0"
entry_point = "main.py"
language = "python"
pages = ["nope"]
"#,
        );
        let tree = ConfigTree::load(&paths).unwrap();
        assert!(tree.plugin_pages.is_empty());
        drop(dir);
    }
}
