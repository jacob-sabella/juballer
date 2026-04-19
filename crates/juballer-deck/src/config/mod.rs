//! Config: schema, paths, loading, hot reload.

pub mod atomic;
pub mod interpolate;
pub mod load;
pub mod paths;
pub mod schema;
pub mod watch;

pub use atomic::atomic_write;
pub use interpolate::{build_env, interpolate};
pub use load::{ConfigTree, ProfileTree};
pub use paths::{default_config_dir, DeckPaths};
pub use schema::*;
pub use watch::{watch, ReloadSignal};
