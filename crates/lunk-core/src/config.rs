use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::errors::{LunkError, Result};

/// Returns the active profile name.
///
/// Resolution order:
/// 1. `LUNK_PROFILE` env var (explicit override)
/// 2. `cfg!(debug_assertions)` → "dev" for debug builds, "default" for release
///
/// The "default" profile uses backwards-compatible paths (e.g. `~/.local/share/lunk/`).
/// Named profiles use `~/.local/share/lunk/profiles/<name>/`.
pub fn active_profile() -> String {
    std::env::var("LUNK_PROFILE").unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            "dev".to_string()
        } else {
            "default".to_string()
        }
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub sync: SyncConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub bind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub enabled: bool,
    pub interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
}

impl Config {
    /// Default config values for the given profile.
    fn defaults_for(profile: &str) -> Self {
        match profile {
            "dev" => Self {
                server: ServerConfig {
                    port: 9724,
                    bind: "127.0.0.1".to_string(),
                },
                sync: SyncConfig {
                    enabled: true,
                    interval_secs: 300,
                },
                logging: LoggingConfig {
                    level: "debug".to_string(),
                },
            },
            _ => Self {
                server: ServerConfig {
                    port: 9723,
                    bind: "127.0.0.1".to_string(),
                },
                sync: SyncConfig {
                    enabled: true,
                    interval_secs: 300,
                },
                logging: LoggingConfig {
                    level: "info".to_string(),
                },
            },
        }
    }

    pub fn load() -> Result<Self> {
        let profile = active_profile();
        let config_path = Self::config_file_path()?;
        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)
                .map_err(|e| LunkError::Config(format!("failed to read config: {e}")))?;
            toml::from_str(&contents)
                .map_err(|e| LunkError::Config(format!("failed to parse config: {e}")))
        } else {
            Ok(Self::defaults_for(&profile))
        }
    }

    pub fn config_file_path() -> Result<PathBuf> {
        let config_dir = Self::config_dir()?;
        Ok(config_dir.join("config.toml"))
    }

    pub fn config_dir() -> Result<PathBuf> {
        let base = project_dirs()?;
        let profile = active_profile();
        if profile == "default" {
            Ok(base.config_dir().to_path_buf())
        } else {
            Ok(base.config_dir().join("profiles").join(&profile))
        }
    }

    /// Data directory for the active profile.
    ///
    /// `LUNK_DATA_DIR` env var overrides everything.
    /// Otherwise, "default" profile uses `~/.local/share/lunk/`,
    /// named profiles use `~/.local/share/lunk/profiles/<name>/`.
    pub fn data_dir() -> Result<PathBuf> {
        if let Ok(dir) = std::env::var("LUNK_DATA_DIR") {
            return Ok(PathBuf::from(dir));
        }
        let base = project_dirs()?;
        let profile = active_profile();
        if profile == "default" {
            Ok(base.data_dir().to_path_buf())
        } else {
            Ok(base.data_dir().join("profiles").join(&profile))
        }
    }

    pub fn db_path() -> Result<PathBuf> {
        let data_dir = Self::data_dir()?;
        Ok(data_dir.join("lunk.db"))
    }

    /// Database path for a specific profile name.
    pub fn db_path_for_profile(profile: &str) -> Result<PathBuf> {
        let base = project_dirs()?;
        let data_dir = if profile == "default" {
            base.data_dir().to_path_buf()
        } else {
            base.data_dir().join("profiles").join(profile)
        };
        Ok(data_dir.join("lunk.db"))
    }

    pub fn secret_key_path() -> Result<PathBuf> {
        let data_dir = Self::data_dir()?;
        Ok(data_dir.join("secret_key"))
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::defaults_for(&active_profile())
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("com", "lunk", "lunk")
        .ok_or_else(|| LunkError::Config("could not determine home directory".to_string()))
}
