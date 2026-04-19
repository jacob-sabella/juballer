//! Action subsystem: trait, registry, built-ins.

mod registry;
mod trait_;

pub use registry::ActionRegistry;
pub use trait_::{Action, ActionCx, ActionKind, BuildFromArgs};

pub mod builtin;
