use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config: {0}")]
    Config(String),

    #[error("config io: {0}")]
    ConfigIo(#[from] std::io::Error),

    #[error("config parse: {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("action registry: unknown action {0}")]
    UnknownAction(String),

    #[error("widget registry: unknown widget {0}")]
    UnknownWidget(String),

    #[error("core: {0}")]
    Core(#[from] juballer_core::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
