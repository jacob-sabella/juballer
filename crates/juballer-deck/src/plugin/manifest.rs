//! Plugin manifest schema (plugins/<name>/manifest.toml).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub entry_point: String,
    /// "python" | "node" | "binary" — informational; deck just exec's `entry_point`.
    pub language: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub widgets: Vec<String>,
    /// Optional list of page names this plugin provides. Each entry `<name>` maps to
    /// `<plugin_dir>/pages/<name>.toml` and is registered as `<plugin_name>:<name>`
    /// in the deck's plugin-page registry.
    #[serde(default)]
    pub pages: Vec<String>,
}

impl PluginManifest {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        toml::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let s = r#"
name = "discord"
version = "0.1.0"
entry_point = "plugin.py"
language = "python"
actions = ["discord.mute", "discord.deafen"]
widgets = ["discord_status"]
"#;
        let m: PluginManifest = toml::from_str(s).unwrap();
        assert_eq!(m.name, "discord");
        assert_eq!(m.actions.len(), 2);
        assert_eq!(m.widgets.len(), 1);
        assert!(m.pages.is_empty());
    }

    #[test]
    fn parse_with_pages() {
        let s = r#"
name = "discord"
version = "0.1.0"
entry_point = "plugin.py"
language = "python"
pages = ["overview", "voice"]
"#;
        let m: PluginManifest = toml::from_str(s).unwrap();
        assert_eq!(m.pages, vec!["overview", "voice"]);
    }
}
