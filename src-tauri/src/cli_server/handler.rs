// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! JSON-RPC request handler for CLI connections.
//!
//! Each CLI connection is a line-delimited JSON-RPC stream.
//! Requests are dispatched to methods in the `methods` module.

use super::protocol::{self, Request, Response};
use super::transport::IpcStream;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::{Duration, timeout};

/// Maximum line length for a single JSON-RPC request (1 MB).
const MAX_LINE_LENGTH: usize = 1_048_576;

/// Idle timeout for a client connection (60 seconds).
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Methods that use streaming (notifications + final response) instead of a single response.
const STREAMING_METHODS: &[&str] = &["ask"];

/// Handle a single CLI client connection.
///
/// Reads lines from the stream, parses JSON-RPC requests,
/// dispatches to the appropriate method, and writes responses.
pub async fn handle_client(stream: IpcStream, app: AppHandle) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();

        // Apply idle timeout and line length limit
        let read_result = timeout(IDLE_TIMEOUT, async {
            let mut bytes_read = 0;
            loop {
                let available = buf_reader.fill_buf().await.map_err(|e| e.to_string())?;
                if available.is_empty() {
                    return Ok::<bool, String>(false); // EOF
                }
                if let Some(newline_pos) = available.iter().position(|&b| b == b'\n') {
                    let chunk = &available[..=newline_pos];
                    bytes_read += chunk.len();
                    if bytes_read > MAX_LINE_LENGTH {
                        return Err("Request too large".to_string());
                    }
                    line.push_str(&String::from_utf8_lossy(chunk));
                    buf_reader.consume(newline_pos + 1);
                    return Ok(true); // Got a complete line
                } else {
                    let len = available.len();
                    bytes_read += len;
                    if bytes_read > MAX_LINE_LENGTH {
                        return Err("Request too large".to_string());
                    }
                    line.push_str(&String::from_utf8_lossy(available));
                    buf_reader.consume(len);
                }
            }
        })
        .await;

        let has_line = match read_result {
            Ok(Ok(true)) => true,
            Ok(Ok(false)) => break, // Client disconnected
            Ok(Err(e)) => {
                tracing::debug!("CLI client read error: {e}");
                break;
            }
            Err(_) => {
                tracing::debug!("CLI client idle timeout");
                break;
            }
        };

        if !has_line {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::err(0, protocol::ERR_INVALID_REQUEST, e.to_string());
                let _ = write_response(&mut writer, &resp).await;
                continue;
            }
        };

        let id = req.id;
        let resp = if STREAMING_METHODS.contains(&req.method.as_str()) {
            // Streaming method: pass writer so method can send notifications
            match super::methods::dispatch_streaming(req.params, &app, &mut writer).await {
                Ok(value) => Response::ok(id, value),
                Err((code, msg)) => Response::err(id, code, msg),
            }
        } else {
            match super::methods::dispatch(&req.method, req.params, &app).await {
                Ok(value) => Response::ok(id, value),
                Err((code, msg)) => Response::err(id, code, msg),
            }
        };

        if write_response(&mut writer, &resp).await.is_err() {
            break; // Client disconnected
        }
    }
}

/// Write a JSON-RPC response as a single line followed by newline.
async fn write_response<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    resp: &Response,
) -> Result<(), std::io::Error> {
    let mut buf = serde_json::to_vec(resp).unwrap_or_default();
    buf.push(b'\n');
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

/// Write a JSON-RPC notification (no `id`) for streaming responses.
pub async fn write_notification<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    method: &str,
    params: serde_json::Value,
) -> Result<(), std::io::Error> {
    let notif = protocol::Notification {
        method: method.to_string(),
        params,
    };
    let mut buf = serde_json::to_vec(&notif).unwrap_or_default();
    buf.push(b'\n');
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}
