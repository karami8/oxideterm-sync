// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! CLI Companion Server
//!
//! Provides a local IPC server (Unix Domain Socket on macOS/Linux,
//! Named Pipe on Windows) that allows the `oxide` CLI binary to
//! communicate with the running OxideTerm GUI process.
//!
//! Protocol: JSON-RPC 2.0, newline-delimited (consistent with agent protocol).

mod handler;
mod methods;
mod protocol;
mod transport;

use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::{Semaphore, oneshot};

pub use protocol::{Notification, Request, Response, RpcError};

/// Maximum concurrent CLI connections.
const MAX_CONNECTIONS: usize = 16;

/// CLI IPC server that bridges the `oxide` CLI with the running GUI.
pub struct CliServer {
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl CliServer {
    /// Start the CLI IPC server.
    ///
    /// This spawns a background task that listens for CLI connections
    /// and dispatches JSON-RPC requests to the appropriate handlers.
    /// The server is non-blocking and does not affect GUI startup.
    pub async fn start(app_handle: AppHandle) -> Result<Arc<Self>, String> {
        let listener = transport::IpcListener::bind()
            .await
            .map_err(|e| format!("CLI server bind failed: {e}"))?;

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let server = Arc::new(Self {
            shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
        });

        tracing::info!(
            "CLI server listening at {}",
            transport::ipc_endpoint_display()
        );

        tokio::spawn(Self::run(listener, app_handle, shutdown_rx));

        Ok(server)
    }

    /// Main accept loop for the IPC server.
    async fn run(
        listener: transport::IpcListener,
        app_handle: AppHandle,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) {
        let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(stream) => {
                            let app = app_handle.clone();
                            let permit = match semaphore.clone().try_acquire_owned() {
                                Ok(permit) => permit,
                                Err(_) => {
                                    tracing::warn!("CLI server: max connections reached, rejecting");
                                    continue;
                                }
                            };
                            tokio::spawn(async move {
                                handler::handle_client(stream, app).await;
                                drop(permit);
                            });
                        }
                        Err(e) => {
                            tracing::warn!("CLI server accept error: {e}");
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    tracing::info!("CLI server shutting down");
                    break;
                }
            }
        }

        // Clean up socket file on Unix
        transport::cleanup();
    }

    /// Shut down the CLI server gracefully.
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.shutdown_tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
            }
        }
    }
}
