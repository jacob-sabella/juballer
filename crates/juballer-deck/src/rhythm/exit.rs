//! Graceful-exit helper — re-export of [`juballer_core::process`].
//!
//! Prefer the canonical path:
//!
//! ```ignore
//! use juballer_core::process::exit;
//! ```

pub use juballer_core::process::{exit, RETURN_ENV};
