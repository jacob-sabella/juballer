//! Preset library: scan, look up, and apply parameter snapshots.
//!
//! Phase 3 ties this into the existing [`super::dispatch::Outcome`]
//! variants so a `load-preset` cell mode actually does something:
//! pressing the cell looks up the preset by name in the library
//! loaded at carla-mode startup, then walks its [[param]] / [[file]]
//! lists and emits OSC writes via the existing [`super::osc`] client.
//!
//! ## Library layout
//!
//! Presets live under `~/.config/juballer/carla/presets/<category>/<name>.preset.toml`.
//! The category is the directory the file sits in, mirroring the
//! rhythm picker's `<pack>/<chart>` convention. Names are unique
//! across the library — a duplicate name in a different category logs
//! a warning and the second occurrence wins (Carla operators tend to
//! name their presets globally; preventing duplicates would surprise
//! users more than overwriting them).

use crate::carla::config::{Preset, PresetParam};
use crate::carla::osc::CarlaClient;
use crate::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One scanned preset file plus the category it lives in.
#[derive(Debug, Clone)]
pub struct PresetEntry {
    pub path: PathBuf,
    pub category: Option<String>,
    pub preset: Preset,
}

impl PresetEntry {
    /// Display name. Falls back to the file stem when `name = "…"` is
    /// missing in the TOML. Strips the conventional `.preset` extension
    /// so `vintage.preset.toml` is reported as `"vintage"`, not
    /// `"vintage.preset"`.
    pub fn name(&self) -> String {
        if let Some(name) = &self.preset.name {
            return name.clone();
        }
        let stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("(unnamed)");
        stem.strip_suffix(".preset").unwrap_or(stem).to_string()
    }
}

/// In-memory index of every preset under the library root, keyed by
/// display name. Built once at carla-mode startup and never mutated;
/// re-scan on demand if the user adds presets while the deck is open.
#[derive(Debug, Default, Clone)]
pub struct PresetLibrary {
    by_name: HashMap<String, PresetEntry>,
}

impl PresetLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_root(root: &Path) -> Self {
        Self {
            by_name: scan_directory(root),
        }
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&PresetEntry> {
        self.by_name.get(name)
    }

    pub fn entries(&self) -> impl Iterator<Item = &PresetEntry> {
        self.by_name.values()
    }

    /// Sorted entries (by case-insensitive name). Used by the preset
    /// picker overlay.
    pub fn sorted(&self) -> Vec<PresetEntry> {
        let mut out: Vec<PresetEntry> = self.by_name.values().cloned().collect();
        out.sort_by_key(|e| e.name().to_lowercase());
        out
    }

    /// Subset filtered by an exact category match.
    pub fn by_category(&self, category: &str) -> Vec<PresetEntry> {
        let mut out: Vec<PresetEntry> = self
            .by_name
            .values()
            .filter(|e| e.category.as_deref() == Some(category))
            .cloned()
            .collect();
        out.sort_by_key(|e| e.name().to_lowercase());
        out
    }

    /// Distinct category names present in the library, sorted.
    pub fn categories(&self) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        for e in self.by_name.values() {
            if let Some(c) = &e.category {
                set.insert(c.clone());
            }
        }
        set.into_iter().collect()
    }
}

/// Recursively walk `root` for `*.preset.toml` files. Each subdirectory
/// becomes a category; files at the root have no category. Files that
/// fail to parse log once and are skipped — a single malformed preset
/// never blocks the rest of the library.
fn scan_directory(root: &Path) -> HashMap<String, PresetEntry> {
    let mut out: HashMap<String, PresetEntry> = HashMap::new();
    walk(root, root, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut HashMap<String, PresetEntry>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::info!(
                target: "juballer::carla::preset",
                "scan {} skipped: {e}",
                dir.display()
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
            continue;
        }
        if !is_preset_file(&path) {
            continue;
        }
        match Preset::load(&path) {
            Ok(preset) => {
                let category = derive_category(root, &path);
                let entry = PresetEntry {
                    path: path.clone(),
                    category,
                    preset,
                };
                let key = entry.name();
                if let Some(prev) = out.insert(key.clone(), entry) {
                    tracing::warn!(
                        target: "juballer::carla::preset",
                        "duplicate preset name {key:?}: {} overrides {}",
                        path.display(),
                        prev.path.display()
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "juballer::carla::preset",
                    "skipping malformed preset {}: {e}",
                    path.display()
                );
            }
        }
    }
}

fn is_preset_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with(".preset.toml"))
}

/// Derive a category name from the relative path between `root` and
/// the preset file. Files immediately under `root` get `None`; files
/// inside `root/<dir>/` get `Some("<dir>")`. Deeper trees are flattened
/// to just the immediate parent so `root/cabs/vintage/marshall` maps
/// to `Some("vintage")` (the most specific category).
fn derive_category(root: &Path, path: &Path) -> Option<String> {
    let parent = path.parent()?;
    if parent == root {
        return None;
    }
    parent
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
}

/// Apply every parameter + custom-data entry in `preset` to Carla.
/// `plugin_override` (set when the binding declares a target plugin
/// different from whatever was named in the preset file) takes
/// priority; otherwise we resolve the plugin against the OSC client
/// using the preset's `target_plugin` field — currently a no-op
/// because Phase 1 plugin resolution only handles indices, so the
/// caller must pass `Some(index)` if they want anything to happen.
pub fn apply(
    client: &CarlaClient,
    entry: &PresetEntry,
    plugin_override: Option<u32>,
) -> Result<usize> {
    let plugin_id = match plugin_override {
        Some(id) => id,
        None => {
            // target_plugin is a string today; we have no name → index
            // map yet (that's Phase 2.1). Skip with a warning.
            tracing::warn!(
                target: "juballer::carla::preset",
                "preset {:?} has no plugin_override and name resolution is not implemented; skipping",
                entry.name()
            );
            return Ok(0);
        }
    };
    let plugin_ref = crate::carla::config::PluginRef::Index(plugin_id);
    let mut emitted = 0usize;
    for PresetParam { name, value } in &entry.preset.params {
        let crate::carla::config::PluginRef::Index(idx) = name else {
            tracing::warn!(
                target: "juballer::carla::preset",
                "preset {:?} param {name:?} is not numeric; skipping (Phase 2.1)",
                entry.name()
            );
            continue;
        };
        client.set_parameter_value(
            &plugin_ref,
            &crate::carla::config::PluginRef::Index(*idx),
            *value,
        );
        emitted += 1;
    }
    for file in &entry.preset.files {
        // Carla expects the value as a string. PathBuf → string lossy
        // conversion preserves UTF-8 paths and replaces the rest with
        // U+FFFD; better than refusing to emit on non-UTF-8 paths.
        let value = file.path.to_string_lossy().into_owned();
        client.set_custom_data(&plugin_ref, "string", &file.key, &value);
        emitted += 1;
    }
    tracing::info!(
        target: "juballer::carla::preset",
        "applied preset {:?} to plugin {plugin_id} ({emitted} writes)",
        entry.name()
    );
    Ok(emitted)
}

/// Default preset library directory.
pub fn default_root() -> PathBuf {
    crate::carla::config::default_presets_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carla::config::{PluginRef, PresetFile, PresetParam};

    fn write_preset(dir: &Path, name: &str, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{name}.preset.toml"));
        std::fs::write(&path, body).unwrap();
        path
    }

    fn minimal(name: &str, target: &str) -> String {
        format!(
            r#"
            name = "{name}"
            target_plugin = "{target}"

            [[param]]
            name = 0
            value = 0.5
            "#
        )
    }

    #[test]
    fn empty_root_yields_empty_library() {
        let dir = std::env::temp_dir().join("juballer-carla-preset-missing-1234");
        let _ = std::fs::remove_dir_all(&dir);
        let lib = PresetLibrary::from_root(&dir);
        assert!(lib.is_empty());
    }

    #[test]
    fn library_walks_categories_and_indexes_by_name() {
        let dir = tempfile::tempdir().unwrap();
        write_preset(
            &dir.path().join("cabs"),
            "vintage",
            &minimal("Vintage", "CabXr"),
        );
        write_preset(
            &dir.path().join("cabs"),
            "modern",
            &minimal("Modern", "CabXr"),
        );
        write_preset(
            &dir.path().join("amps"),
            "clean",
            &minimal("Clean Amp", "Amp"),
        );
        write_preset(dir.path(), "default", &minimal("Default", "Carla-Rack"));

        let lib = PresetLibrary::from_root(dir.path());
        assert_eq!(lib.len(), 4);
        let v = lib.get("Vintage").unwrap();
        assert_eq!(v.category.as_deref(), Some("cabs"));
        assert!(lib.get("Default").unwrap().category.is_none());
        let cats = lib.categories();
        assert_eq!(cats, vec!["amps".to_string(), "cabs".to_string()]);
        let cabs = lib.by_category("cabs");
        assert_eq!(cabs.len(), 2);
        assert_eq!(cabs[0].name(), "Modern", "by_category should be sorted");
    }

    #[test]
    fn malformed_preset_is_skipped_without_taking_others_down() {
        let dir = tempfile::tempdir().unwrap();
        write_preset(dir.path(), "good", &minimal("Good", "Plug"));
        write_preset(dir.path(), "broken", "definitely = invalid = toml");
        let lib = PresetLibrary::from_root(dir.path());
        assert_eq!(lib.len(), 1);
        assert!(lib.get("Good").is_some());
    }

    #[test]
    fn duplicate_name_in_different_categories_overrides_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        write_preset(&dir.path().join("a"), "x", &minimal("Same", "A"));
        write_preset(&dir.path().join("b"), "x", &minimal("Same", "B"));
        let lib = PresetLibrary::from_root(dir.path());
        // Map insertion order is non-deterministic; just confirm the
        // de-dup didn't lose the entry entirely.
        assert_eq!(lib.len(), 1);
    }

    #[test]
    fn sorted_returns_case_insensitive_name_order() {
        let dir = tempfile::tempdir().unwrap();
        write_preset(dir.path(), "z", &minimal("zeta", "X"));
        write_preset(dir.path(), "a", &minimal("Alpha", "X"));
        write_preset(dir.path(), "b", &minimal("beta", "X"));
        let lib = PresetLibrary::from_root(dir.path());
        let names: Vec<String> = lib.sorted().iter().map(PresetEntry::name).collect();
        assert_eq!(names, vec!["Alpha", "beta", "zeta"]);
    }

    #[test]
    fn entry_name_falls_back_to_file_stem_when_field_missing() {
        let dir = tempfile::tempdir().unwrap();
        write_preset(
            dir.path(),
            "no_name",
            r#"
                target_plugin = "X"
            "#,
        );
        let lib = PresetLibrary::from_root(dir.path());
        assert!(lib.get("no_name").is_some());
    }

    fn dummy_entry() -> PresetEntry {
        PresetEntry {
            path: PathBuf::from("/tmp/x.preset.toml"),
            category: None,
            preset: Preset {
                name: Some("X".into()),
                description: None,
                target_plugin: "Plug".into(),
                params: vec![
                    PresetParam {
                        name: PluginRef::Index(0),
                        value: 0.5,
                    },
                    PresetParam {
                        name: PluginRef::Index(2),
                        value: 0.9,
                    },
                ],
                files: vec![PresetFile {
                    key: "ir".into(),
                    path: PathBuf::from("/srv/ir/marshall.wav"),
                }],
            },
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_with_plugin_override_writes_every_param_and_file() {
        // Bind a localhost UDP receiver to stand in for Carla; the OSC
        // client points at it. Apply a preset and assert we see a write
        // for each param + each file.
        let std_recv = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        std_recv.set_nonblocking(true).unwrap();
        let target = std_recv.local_addr().unwrap();
        let recv = tokio::net::UdpSocket::from_std(std_recv).unwrap();

        let rt = tokio::runtime::Handle::current();
        let client = CarlaClient::spawn(&rt, target).unwrap();
        let entry = dummy_entry();
        let emitted = apply(&client, &entry, Some(3)).unwrap();
        assert_eq!(emitted, 3); // 2 params + 1 file

        // Drain three packets and confirm they are addressed at plugin 3.
        let mut bufs: Vec<rosc::OscMessage> = Vec::new();
        let mut buf = [0u8; 1024];
        for _ in 0..3 {
            let (n, _) = tokio::time::timeout(
                std::time::Duration::from_millis(2000),
                recv.recv_from(&mut buf),
            )
            .await
            .expect("packet should arrive within 2s")
            .expect("recv_from ok");
            let (_, pkt) = rosc::decoder::decode_udp(&buf[..n]).unwrap();
            if let rosc::OscPacket::Message(m) = pkt {
                bufs.push(m);
            }
        }
        let addrs: Vec<&str> = bufs.iter().map(|m| m.addr.as_str()).collect();
        assert!(addrs.contains(&"/Carla/3/set_parameter_value"));
        assert!(addrs.contains(&"/Carla/3/set_custom_data"));

        client.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn apply_without_plugin_override_emits_zero_writes_and_warns() {
        let std_recv = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        std_recv.set_nonblocking(true).unwrap();
        let target = std_recv.local_addr().unwrap();
        let _recv = tokio::net::UdpSocket::from_std(std_recv).unwrap();
        let rt = tokio::runtime::Handle::current();
        let client = CarlaClient::spawn(&rt, target).unwrap();
        let emitted = apply(&client, &dummy_entry(), None).unwrap();
        assert_eq!(
            emitted, 0,
            "without name resolution we cannot pick a plugin slot"
        );
        client.shutdown();
    }
}
