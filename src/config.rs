use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::AppError;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Json,
    #[default]
    Markdown,
    Tui,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppConfig {
    pub base_url: String,
    pub token: String,
    #[serde(default)]
    pub preferred_output: OutputMode,
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("base_url", &self.base_url)
            .field("token", &self.masked_token())
            .field("preferred_output", &self.preferred_output)
            .finish()
    }
}

impl AppConfig {
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
        preferred_output: OutputMode,
    ) -> Result<Self, AppError> {
        let base_url = normalize_url(&base_url.into())?;
        let token = token.into().trim().to_string();
        if token.is_empty() {
            return Err(AppError::MissingCredentials);
        }

        Ok(Self {
            base_url,
            token,
            preferred_output,
        })
    }

    pub fn api_url(&self, path: &str) -> String {
        let suffix = path.trim_start_matches('/');
        format!("{}/api/{}", self.base_url.trim_end_matches('/'), suffix)
    }

    pub fn masked_token(&self) -> String {
        if self.token.len() <= 8 {
            return "***".to_string();
        }

        format!("{}...", &self.token[..8])
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionState {
    #[serde(default)]
    pub last_query: String,
    #[serde(default)]
    pub selected_docs: Vec<u64>,
    #[serde(default)]
    pub history: Vec<String>,
}

impl SessionState {
    pub fn push_history(&mut self, command: impl Into<String>) {
        self.history.push(command.into());
        if self.history.len() > 500 {
            let drain = self.history.len() - 500;
            self.history.drain(0..drain);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub config_path: PathBuf,
    pub session_path: PathBuf,
}

impl Default for AppPaths {
    fn default() -> Self {
        let config_root = std::env::var_os("PAPERLESS_CONFIG_PATH")
            .or_else(|| std::env::var_os("PAPERLESS_CLI_CONFIG_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("paperless-cli")
                    .join("config.toml")
            });

        let session_root = std::env::var_os("PAPERLESS_SESSION_PATH")
            .or_else(|| std::env::var_os("PAPERLESS_CLI_SESSION_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::state_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("paperless-cli")
                    .join("session.toml")
            });

        Self {
            config_path: config_root,
            session_path: session_root,
        }
    }
}

impl AppPaths {
    pub fn new(config_path: impl Into<PathBuf>, session_path: impl Into<PathBuf>) -> Self {
        Self {
            config_path: config_path.into(),
            session_path: session_path.into(),
        }
    }

    pub fn config_permissions_restricted(&self) -> bool {
        restricted_permissions(&self.config_path)
    }
}

pub fn normalize_url(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim().trim_end_matches('/');
    let parsed = Url::parse(trimmed).map_err(|_| AppError::InvalidUrl(raw.to_string()))?;

    match parsed.scheme() {
        "https" => Ok(trimmed.to_string()),
        "http" if is_loopback_host(parsed.host_str()) => Ok(trimmed.to_string()),
        "http" => Err(AppError::InsecureRemoteUrl(trimmed.to_string())),
        _ => Err(AppError::InvalidUrl(raw.to_string())),
    }
}

pub fn load_config(paths: &AppPaths) -> Result<AppConfig, AppError> {
    let path = &paths.config_path;
    let persisted = if path.exists() {
        let raw = fs::read_to_string(path)?;
        Some(
            toml::from_str::<AppConfig>(&raw)
                .or_else(|_| serde_json::from_str::<LegacyConfig>(&raw).map(AppConfig::from))
                .map_err(|error| AppError::ConfigMalformed {
                    path: path.clone(),
                    reason: error.to_string(),
                })?,
        )
    } else {
        None
    };

    let base_url = env_value("PAPERLESS_URL")
        .or_else(|| persisted.as_ref().map(|config| config.base_url.clone()));
    let token = env_value("PAPERLESS_TOKEN")
        .or_else(|| persisted.as_ref().map(|config| config.token.clone()));
    let preferred_output = persisted
        .as_ref()
        .map(|config| config.preferred_output)
        .unwrap_or_default();

    match (base_url, token) {
        (Some(base_url), Some(token)) => AppConfig::new(base_url, token, preferred_output),
        (None, None) => Err(AppError::ConfigMissing),
        _ => Err(AppError::Message(
            "Missing Paperless configuration. Set both `PAPERLESS_URL` and `PAPERLESS_TOKEN`, or run `paperless login`.".to_string(),
        )),
    }
}

pub fn save_config(paths: &AppPaths, config: &AppConfig) -> Result<(), AppError> {
    if let Some(parent) = paths.config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    write_private_atomic(&paths.config_path, &toml::to_string_pretty(config)?)?;
    Ok(())
}

pub fn load_session(paths: &AppPaths) -> SessionState {
    let path = &paths.session_path;
    let Some(raw) = fs::read_to_string(path).ok() else {
        return SessionState::default();
    };

    toml::from_str::<SessionState>(&raw).unwrap_or_default()
}

pub fn save_session(paths: &AppPaths, session: &SessionState) -> Result<(), AppError> {
    if let Some(parent) = paths.session_path.parent() {
        fs::create_dir_all(parent)?;
    }

    write_private_atomic(&paths.session_path, &toml::to_string_pretty(session)?)?;
    Ok(())
}

fn write_private_atomic(path: &Path, contents: &str) -> Result<(), AppError> {
    let temp_path = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(&temp_path)?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    drop(file);

    restrict_permissions(&temp_path)?;
    fs::rename(&temp_path, path)?;
    restrict_permissions(path)?;
    Ok(())
}

fn restrict_permissions(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions)?;
    }

    Ok(())
}

fn is_loopback_host(host: Option<&str>) -> bool {
    matches!(host, Some("localhost") | Some("127.0.0.1") | Some("::1"))
}

fn restricted_permissions(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o077 == 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        path.exists()
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Clone, Debug, Deserialize)]
struct LegacyConfig {
    url: String,
    token: String,
}

impl From<LegacyConfig> for AppConfig {
    fn from(value: LegacyConfig) -> Self {
        Self {
            base_url: value.url.trim_end_matches('/').to_string(),
            token: value.token,
            preferred_output: OutputMode::Markdown,
        }
    }
}
