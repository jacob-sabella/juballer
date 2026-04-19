//! Calibration: profile schema, defaults, persistence. Interactive UI lives in `ui.rs`
//! and runs only when the render pipeline is initialized.

mod paths;
mod profile;
mod ui;

pub use paths::default_profile_path;
pub use profile::{GridGeometry, PointPx, Profile, ProfileMeta, SizePx, TopGeometry};
pub use ui::{CalibrationState, Phase};
