//! juballer-core — foundation library for the GAMO2 FB9 controller.
#![forbid(unsafe_op_in_unsafe_fn)]

mod app;
pub mod calibration;
mod error;
mod frame;
pub mod geometry;
pub mod input;
pub mod layout;
pub mod process;
pub mod render;
mod types;
pub mod ui;

pub use app::{App, AppBuilder, PresentMode, RefreshTarget};
pub use calibration::Profile;
pub use error::{Error, Result};
pub use frame::{Frame, GpuCtx, RegionDraw, TileRawCtx};
pub use types::{Color, Rect};
