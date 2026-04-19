#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),

    #[error("profile io: {0}")]
    ProfileIo(#[from] std::io::Error),

    #[error("profile parse: {0}")]
    ProfileParse(#[from] toml::de::Error),

    #[error("gpu init: {0}")]
    GpuInit(String),

    #[error("window: {0}")]
    Window(#[from] winit::error::OsError),

    #[error("event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),

    #[error("input backend: {0}")]
    Input(String),

    #[error("calibration cancelled")]
    CalibrationCancelled,

    #[error("monitor not found: {0}")]
    MonitorNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;
