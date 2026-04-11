// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Cloud sync commands (self-hosted backend friendly)

use crate::commands::config::ConfigState;
use crate::commands::forwarding::ForwardingRegistry;
use crate::config::{ConfigFile, ConnectionOptions, ProxyHopConfig, SavedAuth, SavedConnection};
use crate::forwarding::{ForwardRule as RuntimeForwardRule, ForwardStatus as RuntimeForwardStatus, ForwardType as RuntimeForwardType};
use crate::state::forwarding::{ForwardType as PersistedForwardType, PersistedForward};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tauri::State;
use tracing::info;
use uuid::Uuid;

const SYNC_KEY_ID: &str = "sync:default";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncClientConfig {
    pub backend_url: String,
    #[serde(default = "default_true")]
    pub verify_tls: bool,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub settings_payload: Option<Value>,
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
}

fn default_sync_mode() -> String {
    "push".to_string()
}

fn default_true() -> bool {
    true
}

fn default_timeout_secs() -> u64 {
    15
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub success: bool,
    pub pushed_connections: usize,
    pub pushed_forwards: usize,
    pub pushed_settings_records: usize,
    pub pushed_credentials_records: usize,
    pub pulled_connections: usize,
    pub pulled_forwards: usize,
    pub pulled_settings_records: usize,
    pub pulled_credentials_records: usize,
    pub pulled_settings_payload: Option<Value>,
    pub pulled_settings_deleted: bool,
    pub message: String,
    pub server_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncProxyHopDto {
    host: String,
    port: u16,
    username: String,
    auth_type: String,
    key_path: Option<String>,
    cert_path: Option<String>,
    has_passphrase: bool,
    agent_forwarding: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncConnectionDto {
    id: String,
    name: String,
    group: Option<String>,
    host: String,
    port: u16,
    username: String,
    auth_type: String,
    key_path: Option<String>,
    cert_path: Option<String>,
    has_passphrase: bool,
    color: Option<String>,
    tags: Vec<String>,
    agent_forwarding: bool,
    proxy_chain: Vec<SyncProxyHopDto>,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncForwardDto {
    id: String,
    session_id: String,
    forward_type: String,
    bind_address: String,
    bind_port: u16,
    target_host: String,
    target_port: u16,
    description: Option<String>,
    auto_start: bool,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncSettingsRecordDto {
    id: String,
    payload: Value,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncCredentialsRecordDto {
    id: String,
    version: u32,
    entries: Vec<SyncCredentialEntry>,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPushRequest {
    pushed_at: DateTime<Utc>,
    connections: Vec<SyncConnectionDto>,
    forwards: Vec<SyncForwardDto>,
    #[serde(default)]
    settings_records: Vec<SyncSettingsRecordDto>,
    #[serde(default)]
    credentials_records: Vec<SyncCredentialsRecordDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPullRequest {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncPullResponse {
    #[serde(default)]
    connections: Vec<SyncConnectionDto>,
    #[serde(default)]
    forwards: Vec<SyncForwardDto>,
    #[serde(default)]
    settings_records: Vec<SyncSettingsRecordDto>,
    #[serde(default)]
    credentials_records: Vec<SyncCredentialsRecordDto>,
    server_time: Option<DateTime<Utc>>,
}

fn normalize_base_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

fn build_client(cfg: &SyncClientConfig) -> Result<Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.timeout_secs.max(3)))
        .danger_accept_invalid_certs(!cfg.verify_tls)
        .build()
        .map_err(|e| format!("failed to build sync client: {}", e))
}

fn auth_to_info(auth: &SavedAuth) -> (String, Option<String>, Option<String>, bool) {
    match auth {
        SavedAuth::Password { .. } => ("password".to_string(), None, None, false),
        SavedAuth::Key {
            key_path,
            has_passphrase,
            ..
        } => (
            "key".to_string(),
            Some(key_path.clone()),
            None,
            *has_passphrase,
        ),
        SavedAuth::Certificate {
            key_path,
            cert_path,
            has_passphrase,
            ..
        } => (
            "certificate".to_string(),
            Some(key_path.clone()),
            Some(cert_path.clone()),
            *has_passphrase,
        ),
        SavedAuth::Agent => ("agent".to_string(), None, None, false),
    }
}

fn to_sync_proxy_hop(h: &ProxyHopConfig) -> SyncProxyHopDto {
    let (auth_type, key_path, cert_path, has_passphrase) = auth_to_info(&h.auth);
    SyncProxyHopDto {
        host: h.host.clone(),
        port: h.port,
        username: h.username.clone(),
        auth_type,
        key_path,
        cert_path,
        has_passphrase,
        agent_forwarding: h.agent_forwarding,
    }
}

fn to_sync_connection(c: &SavedConnection) -> SyncConnectionDto {
    let (auth_type, key_path, cert_path, has_passphrase) = auth_to_info(&c.auth);
    SyncConnectionDto {
        id: c.id.clone(),
        name: c.name.clone(),
        group: c.group.clone(),
        host: c.host.clone(),
        port: c.port,
        username: c.username.clone(),
        auth_type,
        key_path,
        cert_path,
        has_passphrase,
        color: c.color.clone(),
        tags: c.tags.clone(),
        agent_forwarding: c.options.agent_forwarding,
        proxy_chain: c.proxy_chain.iter().map(to_sync_proxy_hop).collect(),
        updated_at: c.last_used_at.unwrap_or(c.created_at),
        deleted: c.deleted,
    }
}

fn persisted_type_to_str(t: &PersistedForwardType) -> &'static str {
    match t {
        PersistedForwardType::Local => "local",
        PersistedForwardType::Remote => "remote",
        PersistedForwardType::Dynamic => "dynamic",
    }
}

fn str_to_persisted_type(t: &str) -> PersistedForwardType {
    match t {
        "remote" => PersistedForwardType::Remote,
        "dynamic" => PersistedForwardType::Dynamic,
        _ => PersistedForwardType::Local,
    }
}

fn str_to_runtime_type(t: &str) -> RuntimeForwardType {
    match t {
        "remote" => RuntimeForwardType::Remote,
        "dynamic" => RuntimeForwardType::Dynamic,
        _ => RuntimeForwardType::Local,
    }
}

fn to_sync_forward(f: &PersistedForward) -> SyncForwardDto {
    SyncForwardDto {
        id: f.id.clone(),
        session_id: f.session_id.clone(),
        forward_type: persisted_type_to_str(&f.forward_type).to_string(),
        bind_address: f.rule.bind_address.clone(),
        bind_port: f.rule.bind_port,
        target_host: f.rule.target_host.clone(),
        target_port: f.rule.target_port,
        description: f.rule.description.clone(),
        auto_start: f.auto_start,
        updated_at: f.created_at,
        deleted: false,
    }
}

fn from_sync_connection(dto: SyncConnectionDto) -> SavedConnection {
    let auth = match dto.auth_type.as_str() {
        "key" => SavedAuth::Key {
            key_path: dto.key_path.unwrap_or_default(),
            has_passphrase: dto.has_passphrase,
            passphrase_keychain_id: None,
        },
        "certificate" => SavedAuth::Certificate {
            key_path: dto.key_path.unwrap_or_default(),
            cert_path: dto.cert_path.unwrap_or_default(),
            has_passphrase: dto.has_passphrase,
            passphrase_keychain_id: None,
        },
        "agent" => SavedAuth::Agent,
        _ => SavedAuth::Password { keychain_id: None },
    };

    let proxy_chain = dto
        .proxy_chain
        .into_iter()
        .map(|h| {
            let auth = match h.auth_type.as_str() {
                "key" => SavedAuth::Key {
                    key_path: h.key_path.unwrap_or_default(),
                    has_passphrase: h.has_passphrase,
                    passphrase_keychain_id: None,
                },
                "certificate" => SavedAuth::Certificate {
                    key_path: h.key_path.unwrap_or_default(),
                    cert_path: h.cert_path.unwrap_or_default(),
                    has_passphrase: h.has_passphrase,
                    passphrase_keychain_id: None,
                },
                "agent" => SavedAuth::Agent,
                _ => SavedAuth::Password { keychain_id: None },
            };
            ProxyHopConfig {
                host: h.host,
                port: h.port,
                username: h.username,
                auth,
                agent_forwarding: h.agent_forwarding,
            }
        })
        .collect();

    SavedConnection {
        id: dto.id,
        version: crate::config::CONFIG_VERSION,
        name: dto.name,
        group: dto.group,
        host: dto.host,
        port: dto.port,
        username: dto.username,
        auth,
        options: ConnectionOptions {
            keep_alive_interval: 0,
            compression: false,
            jump_host: None,
            term_type: None,
            agent_forwarding: dto.agent_forwarding,
        },
        created_at: dto.updated_at,
        last_used_at: Some(dto.updated_at),
        color: dto.color,
        tags: dto.tags,
        proxy_chain,
        deleted: dto.deleted,
    }
}

fn from_sync_forward(dto: SyncForwardDto) -> PersistedForward {
    let rule = RuntimeForwardRule {
        id: dto.id.clone(),
        forward_type: str_to_runtime_type(&dto.forward_type),
        bind_address: dto.bind_address,
        bind_port: dto.bind_port,
        target_host: dto.target_host,
        target_port: dto.target_port,
        status: RuntimeForwardStatus::Stopped,
        description: dto.description,
    };

    PersistedForward {
        id: dto.id,
        session_id: dto.session_id,
        owner_connection_id: None,
        forward_type: str_to_persisted_type(&dto.forward_type),
        rule,
        created_at: dto.updated_at,
        auto_start: dto.auto_start,
        version: 1,
    }
}

const SETTINGS_RECORD_ID: &str = "settings/default";
const CREDENTIALS_RECORD_ID: &str = "credentials/default";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncSettingsPayload {
    version: u32,
    groups: Vec<String>,
    recent: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncCredentialEntry {
    key: String,
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncCredentialsPayload {
    version: u32,
    entries: Vec<SyncCredentialEntry>,
}

fn collect_auth_credentials(
    config_state: &ConfigState,
    auth: &SavedAuth,
    key_password: &str,
    key_passphrase: &str,
    entries: &mut Vec<SyncCredentialEntry>,
) {
    match auth {
        SavedAuth::Password {
            keychain_id: Some(keychain_id),
        } => {
            if let Ok(secret) = config_state.get_keychain_value(keychain_id) {
                entries.push(SyncCredentialEntry {
                    key: key_password.to_string(),
                    value: secret,
                });
            }
        }
        SavedAuth::Key {
            has_passphrase: true,
            passphrase_keychain_id: Some(passphrase_keychain_id),
            ..
        }
        | SavedAuth::Certificate {
            has_passphrase: true,
            passphrase_keychain_id: Some(passphrase_keychain_id),
            ..
        } => {
            if let Ok(secret) = config_state.get_keychain_value(passphrase_keychain_id) {
                entries.push(SyncCredentialEntry {
                    key: key_passphrase.to_string(),
                    value: secret,
                });
            }
        }
        _ => {}
    }
}

fn collect_credentials_payload(
    config_state: &ConfigState,
    local_cfg: &ConfigFile,
) -> SyncCredentialsPayload {
    let mut entries = Vec::new();

    for conn in &local_cfg.connections {
        let conn_prefix = format!("conn:{}", conn.id);
        collect_auth_credentials(
            config_state,
            &conn.auth,
            &format!("{}:password", conn_prefix),
            &format!("{}:passphrase", conn_prefix),
            &mut entries,
        );

        for (idx, hop) in conn.proxy_chain.iter().enumerate() {
            let hop_prefix = format!("{}:proxy:{}", conn_prefix, idx);
            collect_auth_credentials(
                config_state,
                &hop.auth,
                &format!("{}:password", hop_prefix),
                &format!("{}:passphrase", hop_prefix),
                &mut entries,
            );
        }
    }

    SyncCredentialsPayload { version: 1, entries }
}

fn upsert_secret(
    target_id: &mut Option<String>,
    secret: &str,
    writes: &mut Vec<(String, String)>,
) {
    let id = target_id
        .clone()
        .unwrap_or_else(|| format!("oxide_sync_{}", Uuid::new_v4()));
    *target_id = Some(id.clone());
    writes.push((id, secret.to_string()));
}

fn apply_auth_credentials(
    auth: &mut SavedAuth,
    key_password: &str,
    key_passphrase: &str,
    entries: &HashMap<String, String>,
    writes: &mut Vec<(String, String)>,
) {
    match auth {
        SavedAuth::Password { keychain_id } => {
            if let Some(secret) = entries.get(key_password) {
                upsert_secret(keychain_id, secret, writes);
            }
        }
        SavedAuth::Key {
            has_passphrase,
            passphrase_keychain_id,
            ..
        }
        | SavedAuth::Certificate {
            has_passphrase,
            passphrase_keychain_id,
            ..
        } => {
            if let Some(secret) = entries.get(key_passphrase) {
                *has_passphrase = true;
                upsert_secret(passphrase_keychain_id, secret, writes);
            }
        }
        SavedAuth::Agent => {}
    }
}

fn apply_credentials_payload(
    config_state: &ConfigState,
    payload: SyncCredentialsPayload,
) -> Result<(), String> {
    let entries_map: HashMap<String, String> = payload
        .entries
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect();

    info!(
        "sync credentials apply start: incoming_entries={}",
        entries_map.len()
    );

    let mut writes: Vec<(String, String)> = Vec::new();

    config_state.update_config(|cfg| {
        for conn in &mut cfg.connections {
            let conn_prefix = format!("conn:{}", conn.id);
            apply_auth_credentials(
                &mut conn.auth,
                &format!("{}:password", conn_prefix),
                &format!("{}:passphrase", conn_prefix),
                &entries_map,
                &mut writes,
            );

            for (idx, hop) in conn.proxy_chain.iter_mut().enumerate() {
                let hop_prefix = format!("{}:proxy:{}", conn_prefix, idx);
                apply_auth_credentials(
                    &mut hop.auth,
                    &format!("{}:password", hop_prefix),
                    &format!("{}:passphrase", hop_prefix),
                    &entries_map,
                    &mut writes,
                );
            }
        }
    })?;

    info!("sync credentials apply computed writes={}", writes.len());

    for (id, secret) in writes {
        config_state.set_keychain_value(&id, &secret)?;
    }

    info!("sync credentials apply finished");

    Ok(())
}

fn clear_auth_credentials(auth: &mut SavedAuth, deletes: &mut Vec<String>) {
    match auth {
        SavedAuth::Password { keychain_id } => {
            if let Some(id) = keychain_id.take() {
                deletes.push(id);
            }
        }
        SavedAuth::Key {
            has_passphrase,
            passphrase_keychain_id,
            ..
        }
        | SavedAuth::Certificate {
            has_passphrase,
            passphrase_keychain_id,
            ..
        } => {
            if let Some(id) = passphrase_keychain_id.take() {
                deletes.push(id);
            }
            *has_passphrase = false;
        }
        SavedAuth::Agent => {}
    }
}

fn clear_credentials_payload(config_state: &ConfigState) -> Result<(), String> {
    let mut deletes = Vec::new();
    config_state.update_config(|cfg| {
        for conn in &mut cfg.connections {
            clear_auth_credentials(&mut conn.auth, &mut deletes);
            for hop in &mut conn.proxy_chain {
                clear_auth_credentials(&mut hop.auth, &mut deletes);
            }
        }
    })?;

    for id in deletes {
        let _ = config_state.delete_keychain_value(&id);
    }

    Ok(())
}

fn should_apply_remote_blob(local_ts: Option<&DateTime<Utc>>, remote_ts: DateTime<Utc>) -> bool {
    match local_ts {
        Some(local) => remote_ts >= *local,
        None => true,
    }
}

fn should_apply_remote_record(
    local_ts: Option<&DateTime<Utc>>,
    remote_ts: DateTime<Utc>,
    sync_mode: &str,
) -> bool {
    if sync_mode == "pull" {
        return true;
    }

    should_apply_remote_blob(local_ts, remote_ts)
}

fn validate_sync_config(cfg: &SyncClientConfig) -> Result<(), String> {
    if cfg.backend_url.trim().is_empty() {
        return Err("backend_url is required".to_string());
    }
    if !["push", "pull"].contains(&cfg.sync_mode.as_str()) {
        return Err("sync_mode must be one of: push, pull".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_set_api_key(
    state: State<'_, std::sync::Arc<ConfigState>>,
    api_key: String,
) -> Result<(), String> {
    if api_key.is_empty() {
        state
            .ai_keychain
            .delete(SYNC_KEY_ID)
            .map_err(|e| format!("failed to delete sync api key: {}", e))?;
        state.api_key_cache.write().remove(SYNC_KEY_ID);
    } else {
        state
            .ai_keychain
            .store(SYNC_KEY_ID, &api_key)
            .map_err(|e| format!("failed to save sync api key: {}", e))?;
        state
            .api_key_cache
            .write()
            .insert(SYNC_KEY_ID.to_string(), api_key);
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_has_api_key(
    state: State<'_, std::sync::Arc<ConfigState>>,
) -> Result<bool, String> {
    Ok(state.ai_keychain.exists(SYNC_KEY_ID).unwrap_or(false))
}

#[tauri::command]
pub async fn sync_delete_api_key(
    state: State<'_, std::sync::Arc<ConfigState>>,
) -> Result<(), String> {
    let _ = state.ai_keychain.delete(SYNC_KEY_ID);
    state.api_key_cache.write().remove(SYNC_KEY_ID);
    Ok(())
}

#[tauri::command]
pub async fn sync_test_connection(
    state: State<'_, std::sync::Arc<ConfigState>>,
    config: SyncClientConfig,
) -> Result<SyncStatus, String> {
    validate_sync_config(&config)?;

    let api_key = state
        .ai_keychain
        .get(SYNC_KEY_ID)
        .map_err(|e| format!("sync api key not found: {}", e))?;

    let client = build_client(&config)?;
    let base = normalize_base_url(&config.backend_url);
    let url = format!("{}/api/v1/sync/health", base);

    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("health check failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("sync backend returned HTTP {}", response.status()));
    }

    Ok(SyncStatus {
        success: true,
        pushed_connections: 0,
        pushed_forwards: 0,
        pushed_settings_records: 0,
        pushed_credentials_records: 0,
        pulled_connections: 0,
        pulled_forwards: 0,
        pulled_settings_records: 0,
        pulled_credentials_records: 0,
        pulled_settings_payload: None,
        pulled_settings_deleted: false,
        message: "连接测试成功".to_string(),
        server_time: None,
    })
}

#[tauri::command]
pub async fn sync_now(
    config_state: State<'_, std::sync::Arc<ConfigState>>,
    forwarding_registry: State<'_, std::sync::Arc<ForwardingRegistry>>,
    config: SyncClientConfig,
) -> Result<SyncStatus, String> {
    validate_sync_config(&config)?;

    let api_key = config_state
        .ai_keychain
        .get(SYNC_KEY_ID)
        .map_err(|e| format!("sync api key not found: {}", e))?;

    let local_cfg = config_state.get_config_snapshot();
    let local_connections: Vec<SyncConnectionDto> =
        local_cfg.connections.iter().map(to_sync_connection).collect();

    let local_forwards = forwarding_registry
        .load_all_persisted_forwards()
        .await?
        .into_iter()
        .map(|f| to_sync_forward(&f))
        .collect::<Vec<_>>();

    let will_push = config.sync_mode != "pull";
    let now = Utc::now();
    let local_settings_records: Vec<SyncSettingsRecordDto> = if will_push {
        config
            .settings_payload
            .clone()
            .map(|payload| {
                vec![SyncSettingsRecordDto {
                    id: SETTINGS_RECORD_ID.to_string(),
                    payload,
                    updated_at: now,
                    deleted: false,
                }]
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let local_credentials_payload = collect_credentials_payload(config_state.inner(), &local_cfg);
    let should_push_credentials =
        will_push && !local_connections.is_empty() && !local_credentials_payload.entries.is_empty();
    let local_credentials_records = if should_push_credentials {
        vec![SyncCredentialsRecordDto {
            id: CREDENTIALS_RECORD_ID.to_string(),
            version: local_credentials_payload.version,
            entries: local_credentials_payload.entries.clone(),
            updated_at: now,
            deleted: false,
        }]
    } else {
        Vec::new()
    };

    info!(
        "sync push payload summary: connections={}, forwards={}, settings_records={}, credentials_entries={}, push_credentials={}",
        local_connections.len(),
        local_forwards.len(),
        local_settings_records.len(),
        local_credentials_payload.entries.len(),
        should_push_credentials
    );

    let local_settings_versions: HashMap<&str, DateTime<Utc>> = local_settings_records
        .iter()
        .map(|record| (record.id.as_str(), record.updated_at))
        .collect();
    let local_credentials_versions: HashMap<&str, DateTime<Utc>> = local_credentials_records
        .iter()
        .map(|record| (record.id.as_str(), record.updated_at))
        .collect();

    let push_payload = SyncPushRequest {
        pushed_at: Utc::now(),
        connections: local_connections.clone(),
        forwards: local_forwards.clone(),
        settings_records: local_settings_records.clone(),
        credentials_records: local_credentials_records.clone(),
    };

    let client = build_client(&config)?;
    let base = normalize_base_url(&config.backend_url);

    let did_push = if will_push {
        let push_url = format!("{}/api/v1/sync/push", base);
        let push_resp = client
            .post(push_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&push_payload)
            .send()
            .await
            .map_err(|e| format!("sync push failed: {}", e))?;

        if !push_resp.status().is_success() {
            return Err(format!("sync push failed with HTTP {}", push_resp.status()));
        }
        true
    } else {
        false
    };

    let pulled_response = if config.sync_mode != "push" {
        let pull_url = format!("{}/api/v1/sync/pull", base);
        let pull_payload = SyncPullRequest {};

        let pull_resp = client
            .post(pull_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&pull_payload)
            .send()
            .await
            .map_err(|e| format!("sync pull failed: {}", e))?;

        if !pull_resp.status().is_success() {
            return Err(format!("sync pull failed with HTTP {}", pull_resp.status()));
        }

        let pulled: SyncPullResponse = pull_resp
            .json()
            .await
            .map_err(|e| format!("invalid sync pull payload: {}", e))?;

        info!(
            "sync pull payload summary: connections={}, forwards={}, settings_records={}, credentials_records={}",
            pulled.connections.len(),
            pulled.forwards.len(),
            pulled.settings_records.len(),
            pulled.credentials_records.len()
        );
        Some(pulled)
    } else {
        None
    };

    let (applied_conn, applied_fw, pulled_settings_records, pulled_credentials_records, pulled_settings_payload, pulled_settings_deleted, server_time) = if let Some(pulled) = pulled_response {
        let server_time = pulled.server_time.clone();
        let mut local_versions: HashMap<String, DateTime<Utc>> = HashMap::new();
        for c in &local_cfg.connections {
            local_versions.insert(c.id.clone(), c.last_used_at.unwrap_or(c.created_at));
        }

        let mut applied_conn = 0usize;
        config_state.update_config(|cfg| {
            for rc in pulled.connections.clone() {
                let should_apply = should_apply_remote_record(
                    local_versions.get(&rc.id),
                    rc.updated_at,
                    &config.sync_mode,
                );

                if !should_apply {
                    continue;
                }

                if rc.deleted {
                    cfg.remove_connection(&rc.id);
                } else {
                    cfg.add_connection(from_sync_connection(rc));
                }
                applied_conn += 1;
            }
        })?;
        config_state.save_config().await?;

        let local_forward_versions: HashMap<String, DateTime<Utc>> = local_forwards
            .iter()
            .map(|f| (f.id.clone(), f.updated_at))
            .collect();

        let mut applied_fw = 0usize;
        for rf in pulled.forwards.clone() {
            let should_apply = should_apply_remote_record(
                local_forward_versions.get(&rf.id),
                rf.updated_at,
                &config.sync_mode,
            );

            if !should_apply {
                continue;
            }

            if rf.deleted {
                let _ = forwarding_registry
                    .delete_persisted_forward(rf.id.clone())
                    .await;
            } else {
                forwarding_registry
                    .persist_forward(from_sync_forward(rf))
                    .await?;
            }
            applied_fw += 1;
        }

        let pulled_settings_records = pulled.settings_records.len();
        let pulled_credentials_records = pulled.credentials_records.len();

        let mut pulled_settings_payload: Option<Value> = None;
        let mut pulled_settings_deleted = false;

        let latest_settings_record = pulled
            .settings_records
            .iter()
            .filter(|r| r.id == SETTINGS_RECORD_ID)
            .max_by_key(|r| r.updated_at);

        if let Some(settings_record) = latest_settings_record {
            let should_apply = should_apply_remote_blob(
                local_settings_versions.get(SETTINGS_RECORD_ID),
                settings_record.updated_at,
            );
            if should_apply {
                if settings_record.deleted {
                    pulled_settings_deleted = true;
                    pulled_settings_payload = None;
                } else {
                    pulled_settings_deleted = false;
                    pulled_settings_payload = Some(settings_record.payload.clone());
                }
            }
        }

        let latest_credentials_record = pulled
            .credentials_records
            .iter()
            .filter(|r| r.id == CREDENTIALS_RECORD_ID)
            .max_by_key(|r| r.updated_at);

        if let Some(credentials_record) = latest_credentials_record {
            let should_apply = should_apply_remote_blob(
                local_credentials_versions.get(CREDENTIALS_RECORD_ID),
                credentials_record.updated_at,
            );

            info!(
                "sync credentials latest record: id={}, deleted={}, entries={}, should_apply={}",
                credentials_record.id,
                credentials_record.deleted,
                credentials_record.entries.len(),
                should_apply
            );

            if should_apply {
                if credentials_record.deleted {
                    clear_credentials_payload(config_state.inner())?;
                } else {
                    apply_credentials_payload(
                        config_state.inner(),
                        SyncCredentialsPayload {
                            version: credentials_record.version,
                            entries: credentials_record.entries.clone(),
                        },
                    )?;
                }
                config_state.save_config().await?;
            }
        }

        (applied_conn, applied_fw, pulled_settings_records, pulled_credentials_records, pulled_settings_payload, pulled_settings_deleted, server_time)
    } else {
        (0, 0, 0, 0, None, false, None)
    };

    let pushed_connections = if did_push { local_connections.len() } else { 0 };
    let pushed_forwards = if did_push { local_forwards.len() } else { 0 };
    let pushed_settings_records = if did_push { local_settings_records.len() } else { 0 };
    let pushed_credentials_records = if did_push { local_credentials_records.len() } else { 0 };

    info!(
        "cloud sync finished: pushed(connections={}, forwards={}, settings_records={}, credentials_records={}) pulled(connections={}, forwards={}, settings_records={}, credentials_records={})",
        pushed_connections,
        pushed_forwards,
        pushed_settings_records,
        pushed_credentials_records,
        applied_conn,
        applied_fw,
        pulled_settings_records,
        pulled_credentials_records,
    );

    Ok(SyncStatus {
        success: true,
        pushed_connections,
        pushed_forwards,
        pushed_settings_records,
        pushed_credentials_records,
        pulled_connections: applied_conn,
        pulled_forwards: applied_fw,
        pulled_settings_records,
        pulled_credentials_records,
        pulled_settings_payload,
        pulled_settings_deleted,
        message: "同步完成".to_string(),
        server_time: server_time.map(|t| t.to_rfc3339()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_should_apply_remote_blob() {
        let now = Utc::now();
        assert!(should_apply_remote_blob(None, now));
        assert!(should_apply_remote_blob(Some(&now), now));
        assert!(should_apply_remote_blob(Some(&now), now + Duration::seconds(1)));
        assert!(!should_apply_remote_blob(Some(&now), now - Duration::seconds(1)));
    }

    #[test]
    fn test_should_apply_remote_record_always_applies_in_pull_mode() {
        let local_newer = Utc::now();
        let remote_older = local_newer - Duration::seconds(1);
        assert!(should_apply_remote_record(
            Some(&local_newer),
            remote_older,
            "pull",
        ));
    }

    #[test]
    fn test_should_apply_remote_record_respects_timestamp_in_push_mode() {
        let local_newer = Utc::now();
        let remote_older = local_newer - Duration::seconds(1);
        assert!(!should_apply_remote_record(
            Some(&local_newer),
            remote_older,
            "push",
        ));
    }

    #[test]
    fn test_from_sync_connection_preserves_auth_metadata() {
        let dto = SyncConnectionDto {
            id: "conn-1".to_string(),
            name: "Conn".to_string(),
            group: Some("g1".to_string()),
            host: "example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth_type: "key".to_string(),
            key_path: Some("/id_ed25519".to_string()),
            cert_path: None,
            has_passphrase: true,
            color: Some("#fff".to_string()),
            tags: vec!["tag1".to_string()],
            agent_forwarding: true,
            proxy_chain: vec![SyncProxyHopDto {
                host: "jump.example.com".to_string(),
                port: 22,
                username: "jump".to_string(),
                auth_type: "certificate".to_string(),
                key_path: Some("/jump_key".to_string()),
                cert_path: Some("/jump_key-cert.pub".to_string()),
                has_passphrase: true,
                agent_forwarding: true,
            }],
            updated_at: Utc::now(),
            deleted: false,
        };

        let conn = from_sync_connection(dto);
        assert!(conn.options.agent_forwarding);

        match conn.auth {
            SavedAuth::Key {
                has_passphrase,
                passphrase_keychain_id,
                ..
            } => {
                assert!(has_passphrase);
                assert!(passphrase_keychain_id.is_none());
            }
            _ => panic!("main auth should be key"),
        }

        assert_eq!(conn.proxy_chain.len(), 1);
        assert!(conn.proxy_chain[0].agent_forwarding);
        match &conn.proxy_chain[0].auth {
            SavedAuth::Certificate {
                has_passphrase,
                passphrase_keychain_id,
                ..
            } => {
                assert!(*has_passphrase);
                assert!(passphrase_keychain_id.is_none());
            }
            _ => panic!("proxy auth should be certificate"),
        }
    }

    #[test]
    fn test_auth_to_info_preserves_agent_and_certificate_metadata() {
        let agent = SavedAuth::Agent;
        let (auth_type, key_path, cert_path, has_passphrase) = auth_to_info(&agent);
        assert_eq!(auth_type, "agent");
        assert!(key_path.is_none());
        assert!(cert_path.is_none());
        assert!(!has_passphrase);

        let cert = SavedAuth::Certificate {
            key_path: "/keys/id_rsa".to_string(),
            cert_path: "/keys/id_rsa-cert.pub".to_string(),
            has_passphrase: true,
            passphrase_keychain_id: Some("kc-1".to_string()),
        };
        let (auth_type, key_path, cert_path, has_passphrase) = auth_to_info(&cert);
        assert_eq!(auth_type, "certificate");
        assert_eq!(key_path.as_deref(), Some("/keys/id_rsa"));
        assert_eq!(cert_path.as_deref(), Some("/keys/id_rsa-cert.pub"));
        assert!(has_passphrase);
    }
}
