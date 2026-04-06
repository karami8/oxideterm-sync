// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

/**
 * Terminal Keyboard Manager
 * 
 * 统一管理终端应用的按键分流机制，解决终端程序（vim, emacs, tmux）
 * 与应用快捷键之间的冲突问题。
 * 
 * 核心原则：
 * 1. 当终端聚焦且没有 UI 面板打开时，大多数按键应传递给终端
 * 2. 只有明确的应用级快捷键（如 Cmd+T 新建标签）才会被拦截
 * 3. Windows 使用 Shift 变体避免与终端程序冲突
 */

import { useEffect, useRef } from 'react';
import { matchAction } from '@/lib/keybindingRegistry';
import { platform } from '../lib/platform';

/**
 * 终端视图专用快捷键 Hook
 * 
 * 用于 TerminalView 和 LocalTerminalView 组件，
 * 只有当该终端是活跃标签时才响应快捷键。
 * 
 * Uses the keybinding registry's matchAction() for key matching,
 * so user overrides are automatically respected.
 */
export function useTerminalViewShortcuts(
  isActive: boolean,
  _isPanelOpen: boolean,
  handlers: {
    onOpenSearch?: () => void;
    onCloseSearch?: () => void;
    onOpenAiPanel?: () => void;
    onCloseAiPanel?: () => void;
    onToggleRecording?: () => void;
    onFocusTerminal?: () => void;
    searchOpen: boolean;
    aiPanelOpen: boolean;
  }
) {
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;
  
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!isActive) return;
      if (!document.hasFocus()) return;
      
      const h = handlersRef.current;
      
      // Match only terminal-scope actions from the registry
      const actionId = matchAction(e, 'terminal');
      if (!actionId) return;
      
      switch (actionId) {
        case 'terminal.search':
          if (h.onOpenSearch) {
            e.preventDefault();
            e.stopPropagation();
            h.onOpenSearch();
          }
          return;

        case 'terminal.aiPanel':
          if (h.onOpenAiPanel) {
            e.preventDefault();
            e.stopPropagation();
            h.onOpenAiPanel();
          }
          return;

        case 'terminal.recording':
          if (h.onToggleRecording) {
            e.preventDefault();
            e.stopPropagation();
            h.onToggleRecording();
          }
          return;

        case 'terminal.closePanel':
          // Only intercept Escape when a panel is open
          if (h.searchOpen && h.onCloseSearch) {
            e.preventDefault();
            e.stopPropagation();
            h.onCloseSearch();
            h.onFocusTerminal?.();
          } else if (h.aiPanelOpen && h.onCloseAiPanel) {
            e.preventDefault();
            e.stopPropagation();
            h.onCloseAiPanel();
            h.onFocusTerminal?.();
          }
          // No panel open: let Escape pass through to terminal
          return;
      }
    };
    
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isActive]);
}

/**
 * 判断按键是否应该被终端捕获（不应被应用拦截）
 * 
 * 这些按键在终端程序中非常重要，不应被应用快捷键覆盖：
 * - Ctrl+C/D/Z: 进程控制
 * - Ctrl+A/E: 行首/行尾（bash/emacs）
 * - Ctrl+R: 反向搜索历史
 * - Ctrl+L: 清屏
 * - Ctrl+U/K/W: 删除操作
 * - Ctrl+P/N: 上一条/下一条命令
 * - Ctrl+B/F: 光标移动
 * - F1-F12: 功能键
 * - Alt+任意键: 终端元键
 */
export function isTerminalReservedKey(e: KeyboardEvent): boolean {
  // F1-F12 功能键 - 永远传递给终端
  if (e.key.startsWith('F') && /^F([1-9]|1[0-2])$/.test(e.key)) {
    return true;
  }
  
  // Alt 组合键 - 终端的元键
  if (e.altKey && !e.ctrlKey && !e.metaKey) {
    return true;
  }
  
  // Ctrl + 单个字母（但不是 Cmd）
  // 这些是终端程序的核心快捷键
  if (e.ctrlKey && !e.metaKey && !e.shiftKey && !e.altKey) {
    const key = e.key.toLowerCase();
    // 保留给终端的 Ctrl+字母
    const terminalCtrlKeys = [
      'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'j', 'k', 'l',
      'n', 'o', 'p', 'q', 'r', 's', 'u', 'v', 'w', 'x', 'y', 'z'
    ];
    if (terminalCtrlKeys.includes(key)) {
      return true;
    }
  }
  
  return false;
}

/**
 * 快捷键帮助文档
 */
export const SHORTCUT_DOCS = {
  app: {
    newTab: platform.isWindows ? 'Ctrl+T' : 'Cmd+T',
    shellLauncher: platform.isWindows ? 'Ctrl+Shift+T' : 'Cmd+Shift+T',
  },
  terminal: {
    search: platform.isWindows ? 'Ctrl+Shift+F' : 'Cmd+F',
    aiPanel: platform.isWindows ? 'Ctrl+Shift+I' : 'Cmd+I',
    closePanel: 'Escape',
  },
} as const;
