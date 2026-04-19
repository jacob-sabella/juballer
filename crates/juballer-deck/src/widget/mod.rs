//! Widget subsystem: trait, registry, built-ins.

pub mod card;
mod registry;
mod trait_;

pub use registry::WidgetRegistry;
pub use trait_::{Widget, WidgetBuildFromArgs, WidgetCx};

pub mod builtin;
