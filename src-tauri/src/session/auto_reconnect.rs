// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Auto Reconnect Service - NEUTRALIZED
//!
//! ⚠️ 此模块已被物理阉割。
//!
//! 设计原则：后端是纯执行器，禁止自主重连决策。
//! 所有重连必须由前端发起，经过 connect_tree_node 入口。
//!
//! 保留此空壳是为了：
//! 1. 兼容 Tauri State 注册（避免编译错误）
//! 2. 提供网络状态查询接口（只读）
//! 3. 保留 cancel_reconnect 接口（前端可调用取消）

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;
use tracing::info;

use super::registry::SessionRegistry;
use crate::commands::forwarding::ForwardingRegistry;

/// Auto reconnect service - NEUTRALIZED STUB
///
/// 🛑 所有自动重连逻辑已被移除
/// 此结构体仅作为 Tauri State 占位符存在
pub struct AutoReconnectService {
    /// Session registry (保留但不使用)
    #[allow(dead_code)]
    registry: Arc<SessionRegistry>,
    /// Forwarding registry (保留但不使用)
    #[allow(dead_code)]
    forwarding_registry: Arc<ForwardingRegistry>,
    /// Tauri app handle (保留但不使用)
    #[allow(dead_code)]
    app_handle: AppHandle,
    /// Global network online status (只读状态)
    network_online: AtomicBool,
}

impl AutoReconnectService {
    /// Create a new auto reconnect service (stub)
    pub fn new(
        registry: Arc<SessionRegistry>,
        forwarding_registry: Arc<ForwardingRegistry>,
        app_handle: AppHandle,
    ) -> Self {
        info!("🛑 AutoReconnectService created as NEUTRALIZED STUB - no auto-reconnect capability");
        Self {
            registry,
            forwarding_registry,
            app_handle,
            network_online: AtomicBool::new(true),
        }
    }

    /// Check if a session is currently reconnecting
    /// 🛑 永远返回 false - 后端不再管理重连状态
    pub fn is_reconnecting(&self, _session_id: &str) -> bool {
        false
    }

    /// Cancel reconnection for a session
    /// 🛑 空操作 - 后端不再有重连任务可取消
    pub fn cancel_reconnect(&self, session_id: &str) {
        info!(
            "🛑 cancel_reconnect called for {} - no-op (service neutralized)",
            session_id
        );
    }

    /// Set network status (只记录状态，不触发任何操作)
    pub fn set_network_status(&self, online: bool) {
        self.network_online.store(online, Ordering::Release);
        info!(
            "Network status updated: online={} (no action taken - service neutralized)",
            online
        );
    }

    /// Check if network is online
    pub fn is_network_online(&self) -> bool {
        self.network_online.load(Ordering::Acquire)
    }

    // 🛑 已移除: trigger_reconnect
    // 🛑 已移除: run_reconnect_loop
    // 🛑 已移除: try_reconnect
    // 🛑 已移除: restore_port_forwards
    // 🛑 已移除: reconnect_all_disconnected
    // 🛑 已移除: pause_all / resume_all
}
