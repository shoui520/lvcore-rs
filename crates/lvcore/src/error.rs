use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid target token")]
    InvalidTargetToken,

    #[error("invalid resource token")]
    InvalidResourceToken,

    #[error("unsupported package family: {0}")]
    UnsupportedFamily(String),

    #[error("package was not recognized")]
    UnrecognizedPackage,

    #[error("book was not found: {0}")]
    BookNotFound(String),

    #[error("package driver error: {0}")]
    Driver(String),
}

pub type Result<T> = std::result::Result<T, Error>;
