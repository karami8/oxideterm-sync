// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Agent Transport — JSON-RPC over SSH exec channel
//!
//! Manages a persistent SSH exec channel to the remote agent process.
//! Sends requests as line-delimited JSON on stdin, reads responses
//! and notifications from stdout.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────┐    JSON lines     ┌─────────────┐
//! │  OxideTerm   │ ───stdin──────▸   │   Agent     │
//! │  Transport   │ ◂──stdout─────    │   Process   │
//! └──────────────┘                    └─────────────┘
//! ```
//!
//! - Requests are matched to responses by `id` field
//! - Notifications (no `id`) are forwarded to a callback
//! - Timeouts prevent indefinite waits

use std::collections::HashMap;
use std::sync::Arc;

use russh::ChannelMsg;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, info, warn};

use super::protocol::*;

/// Default timeout for RPC calls.
const DEFAULT_RPC_TIMEOUT_SECS: u64 = 30;

/// Agent transport error.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Agent not connected")]
    NotConnected,

    #[error("Agent channel closed")]
    ChannelClosed,

    #[error("RPC timeout after {0}s")]
    Timeout(u64),

    #[error("Failed to serialize request: {0}")]
    SerializeError(String),

    #[error("Failed to deserialize response: {0}")]
    DeserializeError(String),

    #[error("Agent RPC error: {0}")]
    RpcError(AgentRpcError),

    #[error("SSH error: {0}")]
    SshError(String),
}

/// Pending RPC handlers (request id → oneshot sender).
type PendingMap =
    Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, AgentRpcError>>>>>;

/// Agent transport — manages JSON-RPC communication over an SSH exec channel.
pub struct AgentTransport {
    /// Send serialized JSON lines to the writer task.
    write_tx: mpsc::Sender<String>,

    /// Pending request map for response matching.
    pending: PendingMap,

    /// Watch event receiver — notifications from the agent.
    watch_rx: Mutex<mpsc::Receiver<WatchEvent>>,

    /// Whether watch_rx has been consumed.
    watch_taken: std::sync::atomic::AtomicBool,

    /// Watch event sender (held by reader task).
    _watch_tx: mpsc::Sender<WatchEvent>,

    /// Shutdown signal.
    shutdown_tx: mpsc::Sender<()>,

    /// Whether the transport is alive.
    alive: Arc<std::sync::atomic::AtomicBool>,
}

impl AgentTransport {
    /// Create a new agent transport from an SSH exec channel.
    ///
    /// This spawns two tasks:
    /// This spawns a combined IO task that handles:
    /// - Writing requests from `write_tx` to channel stdin
    /// - Reading responses/notifications from channel stdout
    pub async fn new(
        mut channel: russh::Channel<russh::client::Msg>,
        agent_command: &str,
    ) -> Result<Self, TransportError> {
        // Execute the agent binary on the remote host
        channel
            .exec(true, agent_command)
            .await
            .map_err(|e| TransportError::SshError(format!("Failed to exec agent: {}", e)))?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let (write_tx, mut write_rx) = mpsc::channel::<String>(256);
        let (watch_tx, watch_rx) = mpsc::channel::<WatchEvent>(1024);
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        // Combined IO task: handles both reading and writing on the single channel
        let pending_r = pending.clone();
        let watch_tx_r = watch_tx.clone();
        let alive_r = alive.clone();
        tokio::spawn(async move {
            let mut buffer = String::new();

            loop {
                tokio::select! {
                    // Write outgoing requests
                    Some(line) = write_rx.recv() => {
                        let data = format!("{}\n", line);
                        if channel.data(data.as_bytes()).await.is_err() {
                            warn!("[agent-transport] write failed, channel closed");
                            break;
                        }
                    }
                    // Read incoming responses/notifications
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) => {
                                // Append to buffer and process complete lines
                                buffer.push_str(&String::from_utf8_lossy(&data));

                                while let Some(newline_pos) = buffer.find('\n') {
                                    let line = buffer[..newline_pos].trim().to_string();
                                    buffer = buffer[newline_pos + 1..].to_string();

                                    if line.is_empty() {
                                        continue;
                                    }

                                    // Try to parse as AgentMessage
                                    match serde_json::from_str::<AgentMessage>(&line) {
                                        Ok(AgentMessage::Response(resp)) => {
                                            let mut pending = pending_r.lock().await;
                                            if let Some(tx) = pending.remove(&resp.id) {
                                                let result = if let Some(err) = resp.error {
                                                    Err(err)
                                                } else {
                                                    Ok(resp.result.unwrap_or_default())
                                                };
                                                let _ = tx.send(result);
                                            } else {
                                                warn!("[agent-transport] Response for unknown id {}", resp.id);
                                            }
                                        }
                                        Ok(AgentMessage::Notification(notif)) => {
                                            if notif.method == "watch/event" {
                                                if let Ok(event) = serde_json::from_value::<WatchEvent>(notif.params) {
                                                    let _ = watch_tx_r.send(event).await;
                                                }
                                            } else {
                                                debug!("[agent-transport] Unknown notification: {}", notif.method);
                                            }
                                        }
                                        Err(e) => {
                                            // Could be stderr data or malformed JSON
                                            debug!("[agent-transport] Non-JSON line: {} ({})", line, e);
                                        }
                                    }
                                }
                            }
                            Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                                // stderr — agent diagnostic output
                                let stderr = String::from_utf8_lossy(&data);
                                for line in stderr.lines() {
                                    debug!("[agent-stderr] {}", line);
                                }
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                info!("[agent-transport] Agent exited with status {}", exit_status);
                                break;
                            }
                            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                                info!("[agent-transport] Channel closed");
                                break;
                            }
                            _ => {
                                // Ignore other messages
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("[agent-transport] Shutdown signal received");
                        break;
                    }
                }
            }

            alive_r.store(false, std::sync::atomic::Ordering::Relaxed);

            // Fail all pending requests
            let mut pending = pending_r.lock().await;
            for (_, tx) in pending.drain() {
                let _ = tx.send(Err(AgentRpcError {
                    code: ERR_INTERNAL,
                    message: "Agent channel closed".to_string(),
                }));
            }

            debug!("[agent-transport] IO task ended");
        });

        Ok(Self {
            write_tx,
            pending,
            watch_rx: Mutex::new(watch_rx),
            watch_taken: std::sync::atomic::AtomicBool::new(false),
            _watch_tx: watch_tx,
            shutdown_tx,
            alive,
        })
    }

    /// Check if the transport is alive.
    pub fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Send an RPC request and wait for the response.
    pub async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        self.call_with_timeout(method, params, DEFAULT_RPC_TIMEOUT_SECS)
            .await
    }

    /// Send an RPC request with a custom timeout.
    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout_secs: u64,
    ) -> Result<serde_json::Value, TransportError> {
        if !self.is_alive() {
            return Err(TransportError::NotConnected);
        }

        let id = next_request_id();
        let request = AgentRequest {
            id,
            method: method.to_string(),
            params,
        };

        let json = serde_json::to_string(&request)
            .map_err(|e| TransportError::SerializeError(e.to_string()))?;

        // Register pending handler
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        // Send to writer task
        self.write_tx
            .send(json)
            .await
            .map_err(|_| TransportError::ChannelClosed)?;

        // Wait for response with timeout
        match tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(rpc_err))) => Err(TransportError::RpcError(rpc_err)),
            Ok(Err(_)) => Err(TransportError::ChannelClosed),
            Err(_) => {
                // Clean up pending entry on timeout
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                Err(TransportError::Timeout(timeout_secs))
            }
        }
    }

    /// Send a fire-and-forget request (no response expected, but still gets one).
    pub async fn notify(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), TransportError> {
        // Agent always sends responses since everything has an `id`,
        // but we can just ignore the response.
        let _ = self.call(method, params).await?;
        Ok(())
    }

    /// Take the watch event receiver.
    ///
    /// Only one consumer should call this. Subsequent calls return None.
    pub async fn take_watch_rx(&self) -> Option<mpsc::Receiver<WatchEvent>> {
        let mut rx = self.watch_rx.lock().await;
        if self
            .watch_taken
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            // Already consumed by a previous caller
            return None;
        }
        // Swap with a closed receiver
        let (_, new_rx) = mpsc::channel(1);
        let old_rx = std::mem::replace(&mut *rx, new_rx);
        Some(old_rx)
    }

    /// Gracefully shut down the agent.
    pub async fn shutdown(&self) {
        // Try to send sys/shutdown
        let _ = self
            .call_with_timeout("sys/shutdown", serde_json::json!({}), 5)
            .await;
        // Signal reader task to stop
        let _ = self.shutdown_tx.send(()).await;
    }
}

impl Drop for AgentTransport {
    fn drop(&mut self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
