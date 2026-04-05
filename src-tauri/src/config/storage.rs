// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Configuration Storage
//!
//! Handles reading/writing configuration files to disk.
//! Config location: ~/.oxideterm on macOS/Linux, %APPDATA%\OxideTerm on Windows
//!
//! Supports configurable data directory via bootstrap.json at the default location.
//! If `~/.oxideterm/bootstrap.json` contains `{ "data_dir": "/custom/path" }`,
//! all data files will be stored at that custom path instead.

use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::types::{CONFIG_VERSION, ConfigFile};

/// Configuration storage errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Failed to determine config directory")]
    NoConfigDir,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Config version {found} is newer than supported {supported}")]
    VersionTooNew { found: u32, supported: u32 },
}

/// Bootstrap configuration stored at the fixed default location.
/// This file controls where the actual data directory lives.
#[derive(serde::Deserialize, serde::Serialize, Default)]
pub struct BootstrapConfig {
    /// Custom data directory path. If None, uses the default location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_dir: Option<String>,
}

impl BootstrapConfig {
    pub fn new_with_data_dir(path: String) -> Self {
        Self {
            data_dir: Some(path),
        }
    }
}

/// Cached resolved data directory path
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Get the default (fixed) OxideTerm directory.
/// This is always the same location regardless of bootstrap config.
/// Bootstrap config file lives here.
pub fn default_dir() -> Result<PathBuf, StorageError> {
    #[cfg(windows)]
    {
        if let Some(app_data) = dirs::config_dir() {
            return Ok(app_data.join("OxideTerm"));
        }
        dirs::home_dir()
            .map(|home| home.join(".oxideterm"))
            .ok_or(StorageError::NoConfigDir)
    }

    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .map(|home| home.join(".oxideterm"))
            .ok_or(StorageError::NoConfigDir)
    }
}

/// Get the bootstrap config file path (always at the default location)
pub fn bootstrap_config_path() -> Result<PathBuf, StorageError> {
    Ok(default_dir()?.join("bootstrap.json"))
}

/// Read the bootstrap config from disk (synchronous, used during init)
fn read_bootstrap_config() -> Option<BootstrapConfig> {
    let path = bootstrap_config_path().ok()?;
    let contents = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&contents) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!("Failed to parse bootstrap.json: {}", e);
            None
        }
    }
}

/// Save bootstrap config to disk (atomic write)
pub fn save_bootstrap_config(config: &BootstrapConfig) -> Result<(), StorageError> {
    let path = bootstrap_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)?;
    let temp_path = path.with_extension("json.tmp");
    std::fs::write(&temp_path, json.as_bytes())?;
    std::fs::rename(&temp_path, &path)?;
    Ok(())
}

/// Get the effective OxideTerm data directory.
/// Checks bootstrap.json for a custom data_dir override, caches result.
/// Returns %APPDATA%\OxideTerm on Windows, ~/.oxideterm on macOS/Linux by default.
pub fn config_dir() -> Result<PathBuf, StorageError> {
    if let Some(cached) = DATA_DIR.get() {
        return Ok(cached.clone());
    }

    let resolved = resolve_data_dir()?;
    Ok(DATA_DIR.get_or_init(|| resolved).clone())
}

/// Resolve the data directory by checking bootstrap config
fn resolve_data_dir() -> Result<PathBuf, StorageError> {
    if let Some(bootstrap) = read_bootstrap_config() {
        if let Some(custom_dir) = bootstrap.data_dir {
            let path = PathBuf::from(&custom_dir);
            if path.is_absolute() {
                tracing::info!("Using custom data directory: {:?}", path);
                return Ok(path);
            }
            tracing::warn!(
                "Ignoring non-absolute data_dir in bootstrap.json: {:?}",
                custom_dir
            );
        }
    }
    default_dir()
}

/// Get the current effective data directory path and whether it's custom
pub fn get_data_dir_info() -> Result<(PathBuf, bool), StorageError> {
    let effective = config_dir()?;
    let default = default_dir()?;
    let is_custom = effective != default;
    Ok((effective, is_custom))
}

/// Get the log directory for storing application logs
pub fn log_dir() -> Result<PathBuf, StorageError> {
    Ok(config_dir()?.join("logs"))
}

/// Get the connections file path
pub fn connections_file() -> Result<PathBuf, StorageError> {
    Ok(config_dir()?.join("connections.json"))
}

/// Configuration storage manager
pub struct ConfigStorage {
    path: PathBuf,
}

impl ConfigStorage {
    /// Create a new storage manager with default path
    pub fn new() -> Result<Self, StorageError> {
        Ok(Self {
            path: connections_file()?,
        })
    }

    /// Create storage manager with custom path (for testing)
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Ensure the config directory exists
    async fn ensure_dir(&self) -> Result<(), StorageError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }

    /// Load configuration from disk
    /// Returns default config if file doesn't exist
    /// If config is corrupted, creates a backup and returns default config
    pub async fn load(&self) -> Result<ConfigFile, StorageError> {
        match fs::read_to_string(&self.path).await {
            Ok(contents) => {
                match serde_json::from_str::<ConfigFile>(&contents) {
                    Ok(config) => {
                        // Check version
                        if config.version > CONFIG_VERSION {
                            return Err(StorageError::VersionTooNew {
                                found: config.version,
                                supported: CONFIG_VERSION,
                            });
                        }
                        // TODO: Run migrations if config.version < CONFIG_VERSION
                        // Currently CONFIG_VERSION == 1, so no migrations needed yet.
                        // When CONFIG_VERSION is bumped, add migration steps here:
                        //
                        // let mut config = config;
                        // if config.version < 2 {
                        //     // migrate v1 → v2: e.g. rename fields, add defaults
                        //     config.version = 2;
                        // }
                        // if config.version < 3 { ... }
                        Ok(config)
                    }
                    Err(e) => {
                        // JSON 解析失败 - 配置文件损坏
                        tracing::warn!("Config file corrupted: {}", e);

                        // 创建备份
                        match self.backup().await {
                            Ok(backup_path) => {
                                tracing::warn!(
                                    "Corrupted config backed up to {:?}, using defaults",
                                    backup_path
                                );
                            }
                            Err(backup_err) => {
                                tracing::error!(
                                    "Failed to backup corrupted config: {}",
                                    backup_err
                                );
                            }
                        }

                        // 返回默认配置
                        Ok(ConfigFile::default())
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    /// Save configuration to disk
    pub async fn save(&self, config: &ConfigFile) -> Result<(), StorageError> {
        self.ensure_dir().await?;

        // Write to temp file first, then rename (atomic write)
        let temp_path = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(config)?;

        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(json.as_bytes()).await?;
        file.sync_all().await?;

        fs::rename(&temp_path, &self.path).await?;

        Ok(())
    }

    /// Check if config file exists
    pub async fn exists(&self) -> bool {
        fs::metadata(&self.path).await.is_ok()
    }

    /// Get config file path
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Create a backup of the current config
    pub async fn backup(&self) -> Result<PathBuf, StorageError> {
        let backup_path = self.path.with_extension(format!(
            "json.backup.{}",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        ));

        if self.exists().await {
            fs::copy(&self.path, &backup_path).await?;
        }

        Ok(backup_path)
    }
}

impl Default for ConfigStorage {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            panic!(
                "Failed to create ConfigStorage with default path: {}. \
                This is likely a system configuration issue.",
                e
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_load_nonexistent() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("test.json");
        let storage = ConfigStorage::with_path(path);

        let config = storage.load().await.unwrap();
        assert_eq!(config.version, CONFIG_VERSION);
        assert!(config.connections.is_empty());
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("test.json");
        let storage = ConfigStorage::with_path(path);

        let mut config = ConfigFile::default();
        config.groups.push("Work".to_string());

        storage.save(&config).await.unwrap();

        let loaded = storage.load().await.unwrap();
        assert_eq!(loaded.groups, vec!["Work"]);
    }
}
