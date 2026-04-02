// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! MCP (Model Context Protocol) Stdio Transport
//!
//! Manages MCP server processes that communicate via stdin/stdout JSON-RPC.
//! Each server is spawned as a child process with configurable command, args, and env.
//!
//! # Concurrency Model
//!
//! Requests to the same MCP server can be sent concurrently. The stdin writer
//! is serialized (necessary for stream ordering), but the response reader runs
//! in a background task that dispatches responses to waiting callers via
//! per-request oneshot channels, keyed by JSON-RPC request ID.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::State;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

// ═══════════════════════════════════════════════════════════════════════════
// State
// ═══════════════════════════════════════════════════════════════════════════

/// Pending response waiters — maps request_id → oneshot sender.
/// The reader task dispatches responses here.
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

struct McpProcess {
    child: Mutex<Child>,
    /// Stdin is serialized: writes must not interleave.
    stdin: Mutex<tokio::process::ChildStdin>,
    /// Monotonic request ID counter (lock-free).
    next_id: AtomicU64,
    /// Pending response waiters.
    pending: PendingMap,
    /// Background task reading stdout and dispatching responses.
    reader_task: JoinHandle<()>,
    /// Background task logging stderr.
    stderr_task: JoinHandle<()>,
}

pub struct McpProcessRegistry {
    processes: Mutex<HashMap<String, Arc<McpProcess>>>,
}

impl McpProcessRegistry {
    pub fn new() -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
        }
    }

    pub async fn stop_all(&self) {
        let mut procs = self.processes.lock().await;
        for (id, proc) in procs.drain() {
            tracing::info!("[MCP] Stopping server {}", id);
            proc.reader_task.abort();
            proc.stderr_task.abort();
            // Reject all pending waiters
            {
                let mut pending = proc.pending.lock().await;
                for (_, tx) in pending.drain() {
                    let _ = tx.send(Err("MCP server shutting down".to_string()));
                }
            }
            let _ = proc.child.lock().await.kill().await;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Stdout Reader Task
// ═══════════════════════════════════════════════════════════════════════════

/// Background task that reads from MCP server stdout using Content-Length
/// framing (per MCP / JSON-RPC over stdio specification).
///
/// Expected wire format:
/// ```text
/// Content-Length: <N>\r\n
/// \r\n
/// <N bytes of JSON>
/// ```
///
/// Falls back to line-delimited JSON for servers that don't send headers.
async fn stdout_reader_loop(
    mut reader: BufReader<tokio::process::ChildStdout>,
    pending: PendingMap,
    server_id: String,
) {
    let mut header_line = String::new();
    loop {
        header_line.clear();
        // Read the first non-empty line — should be "Content-Length: <n>"
        let bytes_read = match reader.read_line(&mut header_line).await {
            Ok(0) => {
                tracing::info!("[MCP:{}] stdout closed", server_id);
                break;
            }
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("[MCP:{}] stdout read error: {}", server_id, e);
                break;
            }
        };

        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            continue; // skip blank lines between messages
        }

        // Try Content-Length framing first
        let body = if let Some(len_str) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            let content_length: usize = match len_str.trim().parse() {
                Ok(n) if n > 0 && n <= 10 * 1024 * 1024 => n, // cap at 10 MB
                Ok(n) => {
                    tracing::warn!("[MCP:{}] Content-Length {} out of range", server_id, n);
                    continue;
                }
                Err(_) => {
                    tracing::debug!(
                        "[MCP:{}] Invalid Content-Length value: {}",
                        server_id,
                        len_str.trim()
                    );
                    continue;
                }
            };

            // Consume remaining headers until empty line (\r\n)
            let mut discard = String::new();
            loop {
                discard.clear();
                match reader.read_line(&mut discard).await {
                    Ok(0) => break,
                    Ok(_) if discard.trim().is_empty() => break,
                    Ok(_) => continue, // skip other headers
                    Err(_) => break,
                }
            }

            // Read exactly content_length bytes
            let mut body_buf = vec![0u8; content_length];
            match tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body_buf).await {
                Ok(_) => String::from_utf8_lossy(&body_buf).into_owned(),
                Err(e) => {
                    tracing::warn!(
                        "[MCP:{}] Failed to read {} body bytes: {}",
                        server_id,
                        content_length,
                        e
                    );
                    continue;
                }
            }
        } else {
            // Fallback: line-delimited JSON (for non-compliant servers)
            let _ = bytes_read;
            trimmed.to_owned()
        };

        let body_trimmed = body.trim();
        if body_trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(body_trimmed) {
            Ok(val) => {
                if let Some(id) = val.get("id").and_then(|v| v.as_u64()) {
                    // This is a response — find and notify the waiter
                    let tx = {
                        let mut map = pending.lock().await;
                        map.remove(&id)
                    };
                    if let Some(tx) = tx {
                        if let Some(error) = val.get("error") {
                            let msg = error
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown MCP error");
                            let _ = tx.send(Err(format!("MCP error: {}", msg)));
                        } else {
                            let result = val.get("result").cloned().unwrap_or(Value::Null);
                            let _ = tx.send(Ok(result));
                        }
                    } else {
                        tracing::warn!(
                            "[MCP:{}] Received response for unknown request id {}",
                            server_id,
                            id
                        );
                    }
                } else {
                    // Server-initiated notification (no id) — log and skip
                    let method = val
                        .get("method")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown");
                    tracing::debug!("[MCP:{}] Server notification: {}", server_id, method);
                }
            }
            Err(e) => {
                tracing::debug!(
                    "[MCP:{}] Non-JSON from stdout: {} — {}",
                    server_id,
                    e,
                    &body_trimmed[..body_trimmed.len().min(100)]
                );
            }
        }
    }

    // Reader exiting — reject all remaining pending waiters
    let mut map = pending.lock().await;
    for (_, tx) in map.drain() {
        let _ = tx.send(Err("MCP server closed stdout".to_string()));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Commands
// ═══════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════
// Command Allowlist — Only these base commands may be spawned as MCP servers
// ═══════════════════════════════════════════════════════════════════════════

/// Allowed MCP server commands. Only the basename is checked (no path traversal).
const MCP_ALLOWED_COMMANDS: &[&str] = &[
    "npx", "node", "python", "python3", "uvx", "uv", "docker", "deno", "bun",
];

/// Validate that the command is in the allowlist and contains no path separators
/// (preventing path-traversal tricks like `../../bin/evil`).
fn validate_mcp_command(command: &str) -> Result<(), String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("MCP command must not be empty".to_string());
    }
    // Extract basename — reject if caller passes an absolute or relative path
    let basename = std::path::Path::new(trimmed)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if basename != trimmed {
        return Err(format!(
            "MCP command must be a plain command name (no paths). Got: '{}'",
            trimmed
        ));
    }
    if !MCP_ALLOWED_COMMANDS.contains(&basename) {
        return Err(format!(
            "MCP command '{}' is not in the allowlist. Allowed: {:?}",
            basename, MCP_ALLOWED_COMMANDS
        ));
    }
    Ok(())
}

/// Validate that no env variable tries to override dangerous variables.
fn validate_mcp_env(env: &HashMap<String, String>) -> Result<(), String> {
    const BLOCKED_ENV_KEYS: &[&str] = &["LD_PRELOAD", "DYLD_INSERT_LIBRARIES", "LD_LIBRARY_PATH"];
    for key in env.keys() {
        if BLOCKED_ENV_KEYS.contains(&key.as_str()) {
            return Err(format!("MCP env variable '{}' is not allowed", key));
        }
    }
    Ok(())
}

/// Spawn an MCP stdio server process. Returns a runtime server ID.
///
/// # Security
/// - Only commands in `MCP_ALLOWED_COMMANDS` may be spawned.
/// - Path separators in the command are rejected (no traversal).
/// - Dangerous env vars (LD_PRELOAD etc.) are blocked.
/// - Child process inherits a cleaned environment with only explicitly passed vars.
#[tauri::command]
pub async fn mcp_spawn_server(
    state: State<'_, Arc<McpProcessRegistry>>,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
) -> Result<String, String> {
    validate_mcp_command(&command)?;
    validate_mcp_env(&env)?;

    let server_id = format!("mcp-{}", uuid::Uuid::new_v4());

    let mut cmd = Command::new(&command);
    cmd.args(&args)
        .env_clear()
        .envs(&env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Inherit PATH so the allowed commands can be found, but only PATH
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    // Inherit HOME for tools that need it (e.g., npx cache)
    if let Ok(home) = std::env::var("HOME") {
        cmd.env("HOME", home);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn MCP server '{}': {}", command, e))?;

    let stdin = child.stdin.take().ok_or("Failed to capture stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;

    // Log stderr in background — tracked so we can cancel on cleanup
    let stderr_task = if let Some(stderr) = child.stderr.take() {
        let sid = server_id.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => tracing::debug!("[MCP:{}] stderr: {}", sid, line.trim_end()),
                    Err(_) => break,
                }
            }
        })
    } else {
        tokio::spawn(async {})
    };

    // Pending response map — shared between writer (registers) and reader (dispatches)
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

    // Spawn the stdout reader task
    let reader_task = {
        let pending_clone = Arc::clone(&pending);
        let sid = server_id.clone();
        tokio::spawn(stdout_reader_loop(
            BufReader::new(stdout),
            pending_clone,
            sid,
        ))
    };

    let proc = Arc::new(McpProcess {
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        next_id: AtomicU64::new(1),
        pending,
        reader_task,
        stderr_task,
    });

    state.processes.lock().await.insert(server_id.clone(), proc);
    tracing::info!("[MCP] Spawned server '{}' as {}", command, server_id);

    Ok(server_id)
}

/// Send a JSON-RPC request to an MCP server and return the result.
///
/// Concurrent requests to the same server are now supported:
/// - Stdin writes are serialized (short critical section)
/// - Response reading is done by a background task
/// - Each caller waits on its own oneshot channel, keyed by request ID
///
/// `params` is a JSON string to avoid Tauri serde issues with generic Value.
#[tauri::command]
pub async fn mcp_send_request(
    state: State<'_, Arc<McpProcessRegistry>>,
    server_id: String,
    method: String,
    params: String,
) -> Result<Value, String> {
    // Clone the Arc so we can release the registry lock immediately
    let proc = {
        let procs = state.processes.lock().await;
        procs
            .get(&server_id)
            .cloned()
            .ok_or_else(|| format!("MCP server {} not found", server_id))?
    };

    // Parse params — return error instead of silently falling back to null
    let params_value: Value =
        serde_json::from_str(&params).map_err(|e| format!("Invalid MCP params JSON: {}", e))?;

    let is_notification = method.starts_with("notifications/");

    // Allocate request ID (lock-free)
    let request_id = proc.next_id.fetch_add(1, Ordering::Relaxed);

    // Build JSON-RPC request
    let request = if is_notification {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params_value,
        })
    } else {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params_value,
        })
    };

    let request_str = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    // For non-notifications, register the waiter BEFORE writing to stdin
    // to avoid a race where the reader task dispatches before we register.
    let rx = if !is_notification {
        let (tx, rx) = oneshot::channel();
        proc.pending.lock().await.insert(request_id, tx);
        Some(rx)
    } else {
        None
    };

    // Write to stdin with Content-Length framing per MCP spec
    {
        let mut stdin = proc.stdin.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", request_str.len());
        stdin
            .write_all(header.as_bytes())
            .await
            .map_err(|e| format!("Failed to write header to MCP server: {}", e))?;
        stdin
            .write_all(request_str.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to MCP server: {}", e))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush: {}", e))?;
    }
    // stdin lock released here — other requests can write immediately

    // For notifications, return immediately
    if is_notification {
        return Ok(Value::Null);
    }

    // Wait for the response from the reader task (with timeout)
    let rx = rx.unwrap(); // Safe: we created it above for non-notifications
    match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            // oneshot dropped — reader task died or server closed
            Err(format!("MCP server {} connection lost", server_id))
        }
        Err(_) => {
            // Timeout — clean up the pending entry
            proc.pending.lock().await.remove(&request_id);
            Err(format!("MCP server {} timed out (30s)", server_id))
        }
    }
}

/// Close an MCP server process.
#[tauri::command]
pub async fn mcp_close_server(
    state: State<'_, Arc<McpProcessRegistry>>,
    server_id: String,
) -> Result<(), String> {
    let proc = {
        let mut procs = state.processes.lock().await;
        procs.remove(&server_id)
    };
    if let Some(proc) = proc {
        tracing::info!("[MCP] Closing server {}", server_id);
        // Send shutdown request (with id per JSON-RPC spec), then exit notification
        {
            let id = proc.next_id.fetch_add(1, Ordering::Relaxed);
            let mut stdin = proc.stdin.lock().await;
            // shutdown is a JSON-RPC request (expects response)
            let shutdown_body = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"shutdown\"}}",
                id
            );
            let shutdown_msg = format!(
                "Content-Length: {}\r\n\r\n{}",
                shutdown_body.len(),
                shutdown_body
            );
            let _ = stdin.write_all(shutdown_msg.as_bytes()).await;
            let _ = stdin.flush().await;
        }
        // Wait briefly for shutdown response, then send exit notification
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            proc.child.lock().await.wait(),
        )
        .await;
        // Send exit notification (no id — it's a notification)
        {
            let mut stdin = proc.stdin.lock().await;
            let exit_body = "{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}";
            let exit_msg = format!("Content-Length: {}\r\n\r\n{}", exit_body.len(), exit_body);
            let _ = stdin.write_all(exit_msg.as_bytes()).await;
            let _ = stdin.flush().await;
        }
        let _ = proc.child.lock().await.kill().await;
        proc.reader_task.abort();
        proc.stderr_task.abort();
        // Reject all remaining pending waiters
        {
            let mut pending = proc.pending.lock().await;
            for (_, tx) in pending.drain() {
                let _ = tx.send(Err("MCP server closed".to_string()));
            }
        }
    }
    Ok(())
}
