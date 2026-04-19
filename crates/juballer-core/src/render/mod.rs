//! Render layer: wgpu init, offscreen FB, composite pass, region drawing.
pub mod composite;
pub mod fill;
pub mod gpu;
pub mod window;

pub use composite::CompositePass;
pub use fill::FillPipeline;
pub use gpu::{Gpu, OffscreenFb};

#[cfg(feature = "headless")]
pub mod headless;
