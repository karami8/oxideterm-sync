import { useEffect, useRef } from 'react';
import type { AgentMode } from './useAgentStatus';
import type { AgentWatchEvent } from '../../../types';
import * as agentService from '../../../lib/agentService';
import { getParentPath, normalizePath } from '../../../lib/pathUtils';
import { triggerGitRefresh, triggerSearchCacheClear, useIdeStore } from '../../../store/ideStore';

const WATCH_RETRY_MS = 3000;
const WATCH_BATCH_MS = 150;

type UseIdeWatchEventsOptions = {
  nodeId: string;
  rootPath: string | undefined;
  enabled: boolean;
  mode: AgentMode;
};

export function useIdeWatchEvents({
  nodeId,
  rootPath,
  enabled,
  mode,
}: UseIdeWatchEventsOptions): void {
  const refreshTreeNode = useIdeStore((state) => state.refreshTreeNode);
  const refreshTreeNodeRef = useRef(refreshTreeNode);

  refreshTreeNodeRef.current = refreshTreeNode;

  useEffect(() => {
    if (!enabled || mode !== 'agent' || !rootPath) {
      return;
    }

    let disposed = false;
    let activeUnlisten: (() => void | Promise<void>) | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let flushTimer: ReturnType<typeof setTimeout> | null = null;
    const pendingRefreshPaths = new Set<string>();
    const normalizedRootPath = normalizePath(rootPath);

    const flushPendingRefreshes = () => {
      flushTimer = null;
      if (pendingRefreshPaths.size === 0) {
        return;
      }

      for (const path of pendingRefreshPaths) {
        refreshTreeNodeRef.current(path);
      }
      pendingRefreshPaths.clear();
      triggerGitRefresh();
      triggerSearchCacheClear();
    };

    const queueRefresh = (event: AgentWatchEvent) => {
      const normalizedEventPath = normalizePath(event.path);
      const refreshPath = normalizedEventPath === normalizedRootPath
        ? normalizedRootPath
        : getParentPath(normalizedEventPath);

      pendingRefreshPaths.add(refreshPath);
      if (!flushTimer) {
        flushTimer = setTimeout(flushPendingRefreshes, WATCH_BATCH_MS);
      }
    };

    const scheduleRetry = () => {
      if (disposed || retryTimer || activeUnlisten) {
        return;
      }

      retryTimer = setTimeout(() => {
        retryTimer = null;
        void startWatching();
      }, WATCH_RETRY_MS);
    };

    const startWatching = async () => {
      if (disposed || activeUnlisten) {
        return;
      }

      const unlisten = await agentService.watchDirectory(
        nodeId,
        normalizedRootPath,
        queueRefresh,
      );

      if (disposed) {
        await unlisten?.();
        return;
      }

      if (!unlisten) {
        scheduleRetry();
        return;
      }

      activeUnlisten = unlisten;
    };

    void startWatching();

    return () => {
      disposed = true;
      if (retryTimer) {
        clearTimeout(retryTimer);
      }
      if (flushTimer) {
        clearTimeout(flushTimer);
      }
      pendingRefreshPaths.clear();

      const unlisten = activeUnlisten;
      activeUnlisten = null;
      void unlisten?.();
    };
  }, [enabled, mode, nodeId, rootPath]);
}