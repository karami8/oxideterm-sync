//! Configuration Management Module
//!
//! Handles persistent storage of connection configurations, SSH config import,
//! and secure credential storage via system keychain.
//!
//! Credential storage:
//! - SSH passwords & passphrases: `com.oxideterm.ssh` keychain service
//! - AI provider API keys: `com.oxideterm.ai` keychain service (since v1.6.0)
//! - Legacy XOR vault files (`ai_keys/*.vault`) are auto-migrated on first access

pub mod keychain;
pub mod ssh_config;
pub mod storage;
pub mod types;
pub mod vault;

#[cfg(target_os = "macos")]
pub mod touch_id;

pub use keychain::{Keychain, KeychainError};
pub use ssh_config::{default_ssh_config_path, parse_ssh_config, SshConfigError, SshConfigHost};
pub use storage::{config_dir, connections_file, ConfigStorage, StorageError};
pub use types::{
    ConfigFile, ConnectionOptions, ProxyHopConfig, SavedAuth, SavedConnection, CONFIG_VERSION,
};
pub use vault::{AiProviderVault, AiVault, VaultError};
