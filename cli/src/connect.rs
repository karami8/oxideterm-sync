// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! IPC connection to the OxideTerm GUI process.
//!
//! - macOS/Linux: Unix Domain Socket at `~/.oxideterm/oxt.sock`
//! - Windows: Named Pipe at `\\.\pipe\OxideTerm-CLI-{username}`

use crate::protocol;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// A connection to the running OxideTerm GUI.
pub struct IpcConnection {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    stream: PipeStream,
}

#[cfg(windows)]
struct PipeStream {
    handle: std::fs::File,
}

impl IpcConnection {
    /// Connect to the running OxideTerm GUI.
    pub fn connect(custom_path: Option<&str>, timeout_ms: u64) -> Result<Self, String> {
        let timeout = Duration::from_millis(timeout_ms);

        #[cfg(unix)]
        {
            let path = if let Some(p) = custom_path {
                std::path::PathBuf::from(p)
            } else if let Ok(p) = std::env::var("OXIDETERM_SOCK") {
                std::path::PathBuf::from(p)
            } else {
                dirs::home_dir()
                    .ok_or("Cannot determine home directory")?
                    .join(".oxideterm")
                    .join("oxt.sock")
            };

            if !path.exists() {
                return Err(format!(
                    "OxideTerm is not running (socket not found: {})\n\
                     Start OxideTerm first, or use 'oxt list connections --offline' for saved data.",
                    path.display()
                ));
            }

            // Verify socket ownership matches current user (prevent interception)
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata =
                    std::fs::metadata(&path).map_err(|e| format!("Cannot stat socket: {e}"))?;
                let socket_uid = metadata.uid();
                let current_uid = unsafe { libc::getuid() };
                if socket_uid != current_uid {
                    return Err(format!(
                        "Socket ownership mismatch: socket owned by uid {socket_uid}, \
                         but current user is uid {current_uid}. \
                         This may indicate a security issue."
                    ));
                }
            }

            let stream = std::os::unix::net::UnixStream::connect(&path)
                .map_err(|e| format!("Failed to connect to OxideTerm: {e}"))?;
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|e| format!("Failed to set timeout: {e}"))?;
            stream
                .set_write_timeout(Some(timeout))
                .map_err(|e| format!("Failed to set timeout: {e}"))?;

            Ok(Self { stream })
        }

        #[cfg(windows)]
        {
            let pipe_name = if let Some(p) = custom_path {
                p.to_string()
            } else if let Ok(p) = std::env::var("OXIDETERM_PIPE") {
                p
            } else {
                format!(r"\\.\pipe\OxideTerm-CLI-{}", whoami::username())
            };

            use std::fs::OpenOptions;
            let handle = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pipe_name)
                .map_err(|e| {
                    format!(
                        "OxideTerm is not running (pipe not found: {pipe_name})\n\
                         Start OxideTerm first. Error: {e}"
                    )
                })?;

            Ok(Self {
                stream: PipeStream { handle },
            })
        }
    }

    /// Send a JSON-RPC request and wait for the response.
    pub fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let req = protocol::Request::new(id, method, params);

        let mut buf = serde_json::to_vec(&req).map_err(|e| format!("Serialize error: {e}"))?;
        buf.push(b'\n');

        self.write_all(&buf)?;
        self.flush()?;

        let line = self.read_line()?;
        let resp: protocol::Response =
            serde_json::from_str(&line).map_err(|e| format!("Invalid response: {e}"))?;

        if let Some(err) = resp.error {
            return Err(format!("[{}] {}", err.code, err.message));
        }

        resp.result
            .ok_or_else(|| "Empty response from server".to_string())
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), String> {
        #[cfg(unix)]
        {
            self.stream
                .write_all(buf)
                .map_err(|e| format!("Write error: {e}"))
        }
        #[cfg(windows)]
        {
            self.stream
                .handle
                .write_all(buf)
                .map_err(|e| format!("Write error: {e}"))
        }
    }

    fn flush(&mut self) -> Result<(), String> {
        #[cfg(unix)]
        {
            self.stream.flush().map_err(|e| format!("Flush error: {e}"))
        }
        #[cfg(windows)]
        {
            self.stream
                .handle
                .flush()
                .map_err(|e| format!("Flush error: {e}"))
        }
    }

    fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        const MAX_RESPONSE: u64 = 4_194_304; // 4 MB limit
        #[cfg(unix)]
        {
            let mut reader = BufReader::new((&self.stream).take(MAX_RESPONSE));
            reader
                .read_line(&mut line)
                .map_err(|e| format!("Read error (is OxideTerm running?): {e}"))?;
        }
        #[cfg(windows)]
        {
            let mut reader = BufReader::new((&self.stream.handle).take(MAX_RESPONSE));
            reader
                .read_line(&mut line)
                .map_err(|e| format!("Read error (is OxideTerm running?): {e}"))?;
        }
        Ok(line)
    }

    /// Send a JSON-RPC request and read streaming notifications.
    ///
    /// The server may send notifications (lines without `id` but with `method`)
    /// before the final response (line with matching `id`). Each notification
    /// with method `stream_chunk` has `params.text` which is passed to `on_chunk`.
    ///
    /// On Unix, the read timeout is temporarily extended to 180s to accommodate
    /// slow first-token latency from AI APIs, then restored when done.
    pub fn call_streaming<F>(
        &mut self,
        method: &str,
        params: serde_json::Value,
        mut on_chunk: F,
    ) -> Result<serde_json::Value, String>
    where
        F: FnMut(&str),
    {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let req = protocol::Request::new(id, method, params);

        let mut buf = serde_json::to_vec(&req).map_err(|e| format!("Serialize error: {e}"))?;
        buf.push(b'\n');
        self.write_all(&buf)?;
        self.flush()?;

        // Extend read timeout for streaming (AI APIs may take >30s for first token)
        const STREAMING_TIMEOUT: Duration = Duration::from_secs(180);
        #[cfg(unix)]
        let original_timeout = self.stream.read_timeout().ok().flatten();
        #[cfg(unix)]
        self.stream
            .set_read_timeout(Some(STREAMING_TIMEOUT))
            .map_err(|e| format!("Failed to set streaming timeout: {e}"))?;

        let result = self.read_streaming_loop(&mut on_chunk);

        // Restore original read timeout
        #[cfg(unix)]
        {
            let _ = self.stream.set_read_timeout(original_timeout);
        }

        result
    }

    fn read_streaming_loop<F>(&mut self, on_chunk: &mut F) -> Result<serde_json::Value, String>
    where
        F: FnMut(&str),
    {
        // Read lines until we get a response with our id
        loop {
            let line = self.read_line()?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as a JSON object
            let obj: serde_json::Value =
                serde_json::from_str(trimmed).map_err(|e| format!("Invalid response: {e}"))?;

            // Check if it's a notification (has "method" but no "id")
            if obj.get("method").is_some() && obj.get("id").is_none() {
                let method_name = obj.get("method").and_then(|v| v.as_str()).unwrap_or("");
                if method_name == "stream_chunk" {
                    if let Some(text) = obj
                        .get("params")
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        on_chunk(text);
                    }
                }
                // Other notifications are silently ignored
                continue;
            }

            // It's a response — parse it
            let resp: protocol::Response =
                serde_json::from_str(trimmed).map_err(|e| format!("Invalid response: {e}"))?;

            if let Some(err) = resp.error {
                return Err(format!("[{}] {}", err.code, err.message));
            }

            return resp.result.ok_or_else(|| "Empty response".to_string());
        }
    }
}
