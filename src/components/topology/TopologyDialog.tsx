// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

/**
 * Topology Dialog Component
 *
 * 显示当前连接拓扑的对话框
 * 
 * Enhanced with:
 * - D3-force layout (prevents node overlap)
 * - Zoom & Pan (navigate large topologies)
 * - Double-click menu (quick actions)
 */

import React, { useState, useRef, useEffect } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { Button } from '../ui/button';
import { useSessionTreeStore } from '../../store/sessionTreeStore';
import { TopologyViewEnhanced } from './TopologyViewEnhanced';
import { buildTopologyTreeCached } from '../../lib/topologyUtils';
import type { TopologyNode } from '../../lib/topologyUtils';
import { Network, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

/**
 * 拓扑图对话框
 */
export const TopologyDialog: React.FC = () => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [tree, setTree] = useState<TopologyNode[]>([]);
  const containerRef = useRef<HTMLDivElement>(null);
  const [dimensions, setDimensions] = useState({ width: 900, height: 500 });

  const { rawNodes } = useSessionTreeStore();

  // Track container size when open
  useEffect(() => {
    if (!open) return;

    const container = containerRef.current;
    if (!container) return;

    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        setDimensions({
          width: entry.contentRect.width,
          height: entry.contentRect.height,
        });
      }
    });

    resizeObserver.observe(container);
    return () => resizeObserver.disconnect();
  }, [open]);

  // 打开对话框时构建树
  const handleOpen = () => {
    // 只显示已连接的节点
    const connectedNodes = rawNodes.filter(
      node => node.state.status === 'connected' || node.state.status === 'connecting'
    );

    const topologyTree = buildTopologyTreeCached(connectedNodes);
    setTree(topologyTree);
    setOpen(true);
  };

  return (
    <>
      {/* 触发按钮 */}
      <Button
        variant="ghost"
        size="icon"
        onClick={handleOpen}
        title={t('topology.button_title')}
        className="rounded-md h-9 w-9"
      >
        <Network className="h-5 w-5" />
      </Button>

      {/* 对话框 */}
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-w-5xl w-full p-0 bg-transparent border border-theme-border rounded-xl overflow-hidden shadow-2xl">
          <DialogHeader className="p-4 bg-theme-bg-panel border-b border-theme-border">
            <DialogTitle className="flex items-center justify-between text-theme-text">
              <div className="flex items-center gap-2">
                <Network className="h-5 w-5 text-theme-accent" />
                <span>{t('topology.title')}</span>
              </div>
              <div className="flex items-center gap-4">
                {tree.length > 0 && (
                  <span className="text-xs font-mono text-theme-text-muted tracking-wider">
                    {t('topology.system_status')} ({getTreeStats(tree, t)})
                  </span>
                )}
                <button
                  onClick={() => setOpen(false)}
                  className="rounded-full p-1 hover:bg-theme-bg text-theme-text-muted hover:text-theme-text transition-colors"
                >
                  <X className="h-5 w-5" />
                </button>
              </div>
            </DialogTitle>
          </DialogHeader>

          {/* 拓扑图容器 - Main Canvas */}
          <div ref={containerRef} className="relative bg-theme-bg min-h-[500px] flex flex-col">

            {/* Legend Overlay */}
            <div className="absolute top-4 left-4 z-10 flex flex-col gap-2 p-3 rounded-lg bg-theme-bg-panel/40 backdrop-blur-sm border border-theme-border/50">
              <div className="text-[10px] uppercase tracking-widest text-theme-text-muted mb-1 font-bold">{t('topology.legend_title')}</div>
              <div className="flex items-center gap-2">
                <div className="w-1.5 h-1.5 rounded-full bg-theme-success shadow-[0_0_8px_var(--theme-success)]" />
                <span className="text-xs text-theme-text">{t('topology.status_active')}</span>
              </div>
              <div className="flex items-center gap-2">
                <div className="w-1.5 h-1.5 rounded-full bg-theme-warning shadow-[0_0_8px_var(--theme-warning)]" />
                <span className="text-xs text-theme-text">{t('topology.status_connecting')}</span>
              </div>
              <div className="flex items-center gap-2">
                <div className="w-1.5 h-1.5 rounded-full bg-theme-text-muted/50" />
                <span className="text-xs text-theme-text-muted">{t('topology.status_idle')}</span>
              </div>
            </div>

            {/* View Component - Enhanced with force layout, zoom/pan, and menus */}
            <TopologyViewEnhanced
              nodes={tree}
              width={dimensions.width}
              height={dimensions.height}
            />

          </div>
        </DialogContent>
      </Dialog>
    </>
  );
};

/**
 * 获取树统计信息
 */
function getTreeStats(nodes: TopologyNode[], t: (key: string, options?: Record<string, unknown>) => string): string {
  let totalNodes = 0;
  let maxDepth = 0;

  function traverse(nodeList: TopologyNode[], depth: number) {
    nodeList.forEach(node => {
      totalNodes++;
      maxDepth = Math.max(maxDepth, depth);

      if (node.children.length > 0) {
        traverse(node.children, depth + 1);
      }
    });
  }

  traverse(nodes, 1);

  if (totalNodes === 1) {
    return t('topology.stats_single');
  }

  return t('topology.stats_multiple', { count: totalNodes, depth: maxDepth });
}
