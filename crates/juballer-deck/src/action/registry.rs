//! Registry: maps action names to factory closures that instantiate actions from TOML args.

use super::trait_::{Action, BuildFromArgs};
use crate::{Error, Result};
use std::collections::HashMap;

pub type ActionFactory = Box<dyn Fn(&toml::Table) -> Result<Box<dyn Action>> + Send + Sync>;

pub struct ActionRegistry {
    factories: HashMap<&'static str, ActionFactory>,
    schemas: HashMap<String, serde_json::Value>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
            schemas: HashMap::new(),
        }
    }

    pub fn register<A>(&mut self, name: &'static str)
    where
        A: Action + BuildFromArgs,
    {
        self.factories.insert(
            name,
            Box::new(|args: &toml::Table| {
                let a = A::from_args(args)?;
                Ok(Box::new(a) as Box<dyn Action>)
            }),
        );
    }

    /// Register an action together with a JSON Schema (Draft-07) describing its args.
    /// The schema is surfaced by [`schema_for`](Self::schema_for) for the editor to
    /// auto-generate config forms.
    pub fn register_with_schema<A>(&mut self, name: &'static str, schema: serde_json::Value)
    where
        A: Action + BuildFromArgs,
    {
        self.register::<A>(name);
        self.schemas.insert(name.to_string(), schema);
    }

    /// Register a custom factory closure under a name. Used by the plugin host to
    /// install per-plugin proxy actions whose closure captures the plugin name.
    pub fn register_factory(&mut self, name: &'static str, factory: ActionFactory) {
        self.factories.insert(name, factory);
    }

    /// Register a custom factory together with a schema. Useful for plugin-provided
    /// actions whose manifest declares the args shape.
    pub fn register_factory_with_schema(
        &mut self,
        name: &'static str,
        factory: ActionFactory,
        schema: serde_json::Value,
    ) {
        self.factories.insert(name, factory);
        self.schemas.insert(name.to_string(), schema);
    }

    pub fn build(&self, name: &str, args: &toml::Table) -> Result<Box<dyn Action>> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| Error::UnknownAction(name.to_string()))?;
        factory(args)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.factories.keys().copied()
    }

    /// Returns the JSON Schema registered alongside `name`, if any. Actions
    /// registered via the plain [`register`](Self::register) path return `None`.
    pub fn schema_for(&self, name: &str) -> Option<serde_json::Value> {
        self.schemas.get(name).cloned()
    }
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, ActionCx};

    struct Echo {
        msg: String,
    }
    impl Action for Echo {
        fn on_down(&mut self, _cx: &mut ActionCx<'_>) {
            let _ = self.msg.len();
        }
    }
    impl BuildFromArgs for Echo {
        fn from_args(args: &toml::Table) -> Result<Self> {
            let msg = args
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("hi")
                .to_string();
            Ok(Self { msg })
        }
    }

    #[test]
    fn register_and_build() {
        let mut r = ActionRegistry::new();
        r.register::<Echo>("test.echo");
        let mut args = toml::Table::new();
        args.insert("msg".into(), toml::Value::String("howdy".into()));
        let _a = r.build("test.echo", &args).unwrap();
        assert!(r.contains("test.echo"));
    }

    #[test]
    fn unknown_action_errors() {
        let r = ActionRegistry::new();
        match r.build("nope", &toml::Table::new()) {
            Err(Error::UnknownAction(name)) => assert_eq!(name, "nope"),
            Err(other) => panic!("wrong variant: {other:?}"),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn schema_for_returns_registered_schema() {
        let mut r = ActionRegistry::new();
        r.register_with_schema::<Echo>(
            "test.echo",
            serde_json::json!({
                "type": "object",
                "properties": {"msg": {"type": "string"}},
            }),
        );
        let s = r.schema_for("test.echo").expect("schema");
        assert_eq!(s["type"], "object");
        assert!(s["properties"]["msg"].is_object());
    }

    #[test]
    fn schema_for_returns_none_without_schema() {
        let mut r = ActionRegistry::new();
        r.register::<Echo>("test.echo");
        assert!(r.schema_for("test.echo").is_none());
    }
}
