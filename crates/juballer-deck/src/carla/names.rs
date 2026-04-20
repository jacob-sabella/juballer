//! Plugin / parameter name → numeric index lookup.
//!
//! Carla addresses everything by integer indices over OSC, but writing
//! `param = 5` in a config TOML is brittle: re-ordering plugins or
//! upgrading them shifts the numbers. The [`NameMap`] absorbs that
//! brittleness — at carla-mode startup we parse the user's
//! `*.carxp` (via [`super::carxp`]) into a name → index lookup, then
//! every [`PluginRef::Name`] / [`ParamRef::Name`] in the dispatch path
//! resolves through here.
//!
//! The map is intentionally keyed by exact string match (not
//! case-insensitive) because Carla's project file preserves the
//! plugin's documented name verbatim — anything fuzzier risks
//! ambiguity when two plugins share a stem.

use crate::carla::carxp::CarlaProject;
use crate::carla::config::{ParamRef, PluginRef};
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct NameMap {
    /// `plugin_name -> slot index`.
    plugins: HashMap<String, u32>,
    /// `(plugin_slot, param_name) -> param_index`. Indexed by the
    /// plugin's resolved slot rather than its name so a single plugin
    /// loaded twice (which Carla allows) keeps two distinct param
    /// sub-maps.
    params: HashMap<u32, HashMap<String, u32>>,
}

impl NameMap {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a NameMap from a parsed [`CarlaProject`].
    pub fn from_project(project: &CarlaProject) -> Self {
        let mut map = Self::default();
        for plugin in &project.plugins {
            map.plugins.insert(plugin.name.clone(), plugin.slot);
            let mut sub = HashMap::with_capacity(plugin.params.len());
            for param in &plugin.params {
                sub.insert(param.name.clone(), param.index);
                if let Some(symbol) = &param.symbol {
                    if symbol != &param.name {
                        sub.insert(symbol.clone(), param.index);
                    }
                }
            }
            map.params.insert(plugin.slot, sub);
        }
        map
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn param_count(&self) -> usize {
        self.params.values().map(HashMap::len).sum()
    }

    /// Resolve a plugin reference to its numeric slot. Names that
    /// don't appear in the map return `None`; numeric refs pass
    /// through unchanged.
    pub fn resolve_plugin(&self, plugin: &PluginRef) -> Option<u32> {
        match plugin {
            PluginRef::Index(i) => Some(*i),
            PluginRef::Name(n) => self.plugins.get(n).copied(),
        }
    }

    /// Resolve a parameter reference. Requires the resolved plugin
    /// slot since the param-name lookup is scoped per-plugin.
    pub fn resolve_param(&self, plugin_slot: u32, param: &ParamRef) -> Option<u32> {
        match param {
            ParamRef::Index(i) => Some(*i),
            ParamRef::Name(n) => self.params.get(&plugin_slot)?.get(n).copied(),
        }
    }

    /// Reverse lookup — slot index → name, useful for HUD breadcrumbs.
    pub fn plugin_name_for(&self, slot: u32) -> Option<&str> {
        self.plugins
            .iter()
            .find(|(_, idx)| **idx == slot)
            .map(|(name, _)| name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carla::carxp::{CarlaProject, ProjectParam, ProjectPlugin};

    fn fixture() -> CarlaProject {
        CarlaProject {
            plugins: vec![
                ProjectPlugin {
                    slot: 0,
                    name: "ZynReverb".into(),
                    plugin_type: Some("INTERNAL".into()),
                    uri: None,
                    label: Some("zynreverb".into()),
                    params: vec![
                        ProjectParam {
                            index: 0,
                            name: "Time".into(),
                            symbol: None,
                        },
                        ProjectParam {
                            index: 5,
                            name: "Low-Pass Filter".into(),
                            symbol: None,
                        },
                    ],
                },
                ProjectPlugin {
                    slot: 1,
                    name: "GxTuner".into(),
                    plugin_type: Some("LV2".into()),
                    uri: Some("http://gx".into()),
                    label: None,
                    params: vec![
                        ProjectParam {
                            index: 0,
                            name: "FREQ".into(),
                            symbol: Some("FREQ".into()),
                        },
                        ProjectParam {
                            index: 5,
                            name: "THRESHOLD".into(),
                            symbol: Some("THRESHOLD".into()),
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn empty_map_resolves_indices_but_returns_none_for_names() {
        let map = NameMap::empty();
        assert_eq!(map.resolve_plugin(&PluginRef::Index(7)), Some(7));
        assert_eq!(
            map.resolve_plugin(&PluginRef::Name("Anything".into())),
            None
        );
    }

    #[test]
    fn from_project_builds_plugin_name_index() {
        let map = NameMap::from_project(&fixture());
        assert_eq!(map.plugin_count(), 2);
        assert_eq!(
            map.resolve_plugin(&PluginRef::Name("ZynReverb".into())),
            Some(0)
        );
        assert_eq!(
            map.resolve_plugin(&PluginRef::Name("GxTuner".into())),
            Some(1)
        );
    }

    #[test]
    fn resolve_param_scopes_to_plugin_slot() {
        let map = NameMap::from_project(&fixture());
        // Both plugins have a param at index 0 with different names.
        assert_eq!(
            map.resolve_param(0, &ParamRef::Name("Time".into())),
            Some(0)
        );
        assert_eq!(
            map.resolve_param(1, &ParamRef::Name("FREQ".into())),
            Some(0)
        );
        // Wrong plugin scope returns None.
        assert_eq!(map.resolve_param(0, &ParamRef::Name("FREQ".into())), None);
    }

    #[test]
    fn resolve_param_falls_back_to_index_for_index_variant() {
        let map = NameMap::empty();
        assert_eq!(map.resolve_param(99, &ParamRef::Index(42)), Some(42));
    }

    #[test]
    fn from_project_indexes_lv2_symbol_when_distinct_from_name() {
        let project = CarlaProject {
            plugins: vec![ProjectPlugin {
                slot: 0,
                name: "X".into(),
                plugin_type: None,
                uri: None,
                label: None,
                params: vec![ProjectParam {
                    index: 7,
                    name: "Wet Mix".into(),
                    symbol: Some("wet_mix".into()),
                }],
            }],
        };
        let map = NameMap::from_project(&project);
        assert_eq!(
            map.resolve_param(0, &ParamRef::Name("Wet Mix".into())),
            Some(7),
            "name should resolve"
        );
        assert_eq!(
            map.resolve_param(0, &ParamRef::Name("wet_mix".into())),
            Some(7),
            "LV2 symbol should also resolve when distinct from the name"
        );
    }

    #[test]
    fn plugin_name_for_inverts_the_index_lookup() {
        let map = NameMap::from_project(&fixture());
        assert_eq!(map.plugin_name_for(0), Some("ZynReverb"));
        assert_eq!(map.plugin_name_for(1), Some("GxTuner"));
        assert_eq!(map.plugin_name_for(99), None);
    }
}
