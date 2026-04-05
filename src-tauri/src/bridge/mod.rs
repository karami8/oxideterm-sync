// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! WebSocket Bridge module
//!
//! This module provides WebSocket server functionality for bridging
//! SSH sessions to the frontend xterm.js terminal.

mod manager;
mod protocol;
mod server;

pub use manager::BridgeManager;
pub use protocol::{
    Frame, FrameCodec, MessageType, data_frame, error_frame, heartbeat_frame, resize_frame,
};
pub use server::{DisconnectReason, WsBridge};
