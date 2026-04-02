// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Local terminal support module
//!
//! Provides PTY-based local terminal functionality, allowing OxideTerm
//! to function as a standalone terminal emulator without SSH.

pub mod pty;
pub mod registry;
pub mod session;
pub mod shell;

pub use pty::{PtyConfig, PtyError, PtyHandle};
pub use registry::LocalTerminalRegistry;
pub use session::{
    BackgroundSessionInfo, LocalTerminalInfo, LocalTerminalSession, SessionError, SessionEvent,
};
pub use shell::{default_shell, get_shell_args, scan_shells, ShellInfo};
