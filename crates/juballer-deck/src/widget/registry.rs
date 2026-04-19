use super::trait_::{Widget, WidgetBuildFromArgs};
use crate::{Error, Result};
use std::collections::HashMap;

pub type WidgetFactory = Box<dyn Fn(&toml::Table) -> Result<Box<dyn Widget>> + Send + Sync>;

pub struct WidgetRegistry {
    factories: HashMap<&'static str, WidgetFactory>,
    schemas: HashMap<String, serde_json::Value>,
}

impl WidgetRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
            schemas: HashMap::new(),
        }
    }

    pub fn register<W>(&mut self, name: &'static str)
    where
        W: Widget + WidgetBuildFromArgs,
    {
        self.factories.insert(
            name,
            Box::new(|args: &toml::Table| Ok(Box::new(W::from_args(args)?) as Box<dyn Widget>)),
        );
    }

    /// Register a widget together with a JSON Schema (Draft-07) describing its args.
    /// The schema is surfaced by [`schema_for`](Self::schema_for) for the editor to
    /// auto-generate config forms.
    pub fn register_with_schema<W>(&mut self, name: &'static str, schema: serde_json::Value)
    where
        W: Widget + WidgetBuildFromArgs,
    {
        self.register::<W>(name);
        self.schemas.insert(name.to_string(), schema);
    }

    pub fn build(&self, name: &str, args: &toml::Table) -> Result<Box<dyn Widget>> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| Error::UnknownWidget(name.to_string()))?;
        factory(args)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.factories.keys().copied()
    }

    /// Returns the JSON Schema registered alongside `name`, if any. Widgets registered
    /// via the plain [`register`](Self::register) path return `None`.
    pub fn schema_for(&self, name: &str) -> Option<serde_json::Value> {
        self.schemas.get(name).cloned()
    }
}

impl Default for WidgetRegistry {
    fn default() -> Self {
        Self::new()
    }
}
