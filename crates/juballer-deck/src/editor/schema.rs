//! JSON Schema helpers for the editor server.
//!
//! Action and widget schemas are owned by the registries (see
//! `crate::action::ActionRegistry::schema_for` /
//! `crate::widget::WidgetRegistry::schema_for`); this module only owns the
//! placeholder used when a registered name has no schema attached yet.

use serde_json::json;

/// Empty draft-07 object schema. Returned by the editor for actions / widgets that
/// have no schema attached at the registry level.
pub fn empty_schema() -> serde_json::Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {},
    })
}
