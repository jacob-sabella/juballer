//! The rhythm game's core: memon charts, judging and game state.
//!
//! Pure model code with no audio, graphics or input dependency, so hosts other
//! than `juballer-deck` can drive it.

pub mod chart;
pub mod judge;
pub mod notes;
pub mod state;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),

    #[error("config io: {0}")]
    ConfigIo(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
