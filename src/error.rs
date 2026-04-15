use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Paperless is not configured. Run `paperless login`.")]
    ConfigMissing,
    #[error("Config at {path} is malformed: {reason}")]
    ConfigMalformed { path: PathBuf, reason: String },
    #[error("Invalid Paperless URL: {0}")]
    InvalidUrl(String),
    #[error("Refusing insecure remote HTTP URL: {0}. Use HTTPS or a local loopback address.")]
    InsecureRemoteUrl(String),
    #[error("Provide either a token or both username and password.")]
    MissingCredentials,
    #[error("No fields supplied for update.")]
    NoFieldsToUpdate,
    #[error("File not found: {0}")]
    FileMissing(PathBuf),
    #[error("HTTP {status} from {url}: {message}")]
    Http {
        status: u16,
        url: String,
        message: String,
    },
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    TomlDeserialize(#[from] toml::de::Error),
    #[error(transparent)]
    TomlSerialize(#[from] toml::ser::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}
