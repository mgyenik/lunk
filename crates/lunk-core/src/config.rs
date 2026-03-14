use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::errors::{LunkError, Result};

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
    pub crsqlite_ext_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                port: 9723,
                bind: "127.0.0.1".to_string(),
            },
            sync: SyncConfig {
                enabled: true,
                interval_secs: 300,
                crsqlite_ext_path: None,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
            },
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_file_path()?;
        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)
                .map_err(|e| LunkError::Config(format!("failed to read config: {e}")))?;
            toml::from_str(&contents)
                .map_err(|e| LunkError::Config(format!("failed to parse config: {e}")))
        } else {
            Ok(Self::default())
        }
    }

    pub fn config_file_path() -> Result<PathBuf> {
        let config_dir = Self::config_dir()?;
        Ok(config_dir.join("config.toml"))
    }

    pub fn config_dir() -> Result<PathBuf> {
        let dirs = project_dirs()?;
        Ok(dirs.config_dir().to_path_buf())
    }

    pub fn data_dir() -> Result<PathBuf> {
        let dirs = project_dirs()?;
        Ok(dirs.data_dir().to_path_buf())
    }

    pub fn db_path() -> Result<PathBuf> {
        let data_dir = Self::data_dir()?;
        Ok(data_dir.join("lunk.db"))
    }

    pub fn secret_key_path() -> Result<PathBuf> {
        let data_dir = Self::data_dir()?;
        Ok(data_dir.join("secret_key"))
    }
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("com", "lunk", "lunk")
        .ok_or_else(|| LunkError::Config("could not determine home directory".to_string()))
}
