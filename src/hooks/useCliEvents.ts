/**
 * Hook to listen for CLI-triggered events from the backend.
 *
 * Events:
 *   - cli:connect     → connect to a saved connection by ID
 *   - cli:open-tab    → open a new local terminal tab (optionally with cwd)
 *   - cli:focus-tab   → focus an existing tab by session_id
 */

import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/appStore';
import { useLocalTerminalStore } from '../store/localTerminalStore';
import { connectToSaved } from '../lib/connectToSaved';
import { useToastStore } from './useToast';
import { useTranslation } from 'react-i18next';
import type { PaneNode } from '../types';

interface CliConnectPayload {
  connection_id: string;
  name: string;
  host: string;
}

interface CliOpenTabPayload {
  path: string | null;
}

interface CliFocusTabPayload {
  session_id?: string;
  target?: string;
}

export function useCliEvents(): void {
  const { t } = useTranslation('connections');

  useEffect(() => {
    const listeners: (() => void)[] = [];

    const setup = async () => {
      // cli:connect — trigger connectToSaved flow
      const unlistenConnect = await listen<CliConnectPayload>('cli:connect', (event) => {
        const { connection_id, name } = event.payload;
        console.info('[CLI] cli:connect received', connection_id, name);

        const { createTab } = useAppStore.getState();
        const { addToast } = useToastStore.getState();

        connectToSaved(connection_id, {
          createTab: (type, sessionId) => createTab(type, sessionId),
          toast: ({ title, description, variant }) =>
            addToast({ title, description, variant: variant ?? 'default' }),
          t: (key, options) => t(key, options as Record<string, string>),
        }).catch((err) => {
          console.error('[CLI] cli:connect failed', connection_id, err);
          addToast({
            title: t('toast.connect_failed', { ns: 'connections' }),
            description: String(err),
            variant: 'error',
          });
        });
      });
      listeners.push(unlistenConnect);

      // cli:open-tab — open a new local terminal
      const unlistenOpen = await listen<CliOpenTabPayload>('cli:open-tab', async (event) => {
        const { path } = event.payload;
        console.info('[CLI] cli:open-tab received', path);

        try {
          const info = await useLocalTerminalStore.getState().createTerminal(
            path ? { cwd: path } : undefined,
          );
          useAppStore.getState().createTab('local_terminal', info.id);
        } catch (err) {
          console.error('[CLI] cli:open-tab failed', err);
        }
      });
      listeners.push(unlistenOpen);

      // cli:focus-tab — focus an existing tab by session ID or target string
      const unlistenFocus = await listen<CliFocusTabPayload>('cli:focus-tab', (event) => {
        const { session_id, target } = event.payload;
        console.info('[CLI] cli:focus-tab received', session_id ?? target);

        const { tabs, setActiveTab } = useAppStore.getState();

        let tab;

        if (session_id) {
          // Match by sessionId (direct or in split-pane rootPane tree)
          tab = tabs.find((t) => t.sessionId === session_id)
            ?? tabs.find((t) => t.rootPane != null && paneTreeContainsSession(t.rootPane, session_id));
        }

        if (!tab && target) {
          // Match by tab id, then by title (case-insensitive)
          const lower = target.toLowerCase();
          tab = tabs.find((t) => t.id === target)
            ?? tabs.find((t) => t.title.toLowerCase() === lower)
            ?? tabs.find((t) => t.title.toLowerCase().includes(lower));
        }

        if (tab) {
          setActiveTab(tab.id);
        } else {
          console.warn('[CLI] cli:focus-tab: no tab found for', session_id ?? target);
        }
      });
      listeners.push(unlistenFocus);
    };

    setup();

    return () => {
      listeners.forEach((unlisten) => unlisten());
    };
  }, [t]);
}

/** Recursively check if a pane tree contains a leaf with the given sessionId. */
function paneTreeContainsSession(node: PaneNode, sessionId: string): boolean {
  if (node.type === 'leaf') {
    return node.sessionId === sessionId;
  }
  return node.children.some((child) => paneTreeContainsSession(child, sessionId));
}
