import { beforeEach, describe, expect, it, vi } from 'vitest';

import { usePluginStore } from '@/store/pluginStore';
import type { PluginManifest } from '@/types/plugin';

function manifest(overrides: Partial<PluginManifest> = {}): PluginManifest {
  return {
    id: 'plugin.example',
    name: 'Example Plugin',
    version: '1.0.0',
    main: 'dist/index.js',
    contributes: {
      tabs: [{ id: 'tab', title: 'Example', icon: 'Puzzle' }],
      sidebarPanels: [{ id: 'panel', title: 'Panel', icon: 'Puzzle', position: 'bottom' }],
    },
    ...overrides,
  };
}

function resetPluginStore() {
  usePluginStore.setState({
    plugins: new Map(),
    tabViews: new Map(),
    sidebarPanels: new Map(),
    inputInterceptors: [],
    outputProcessors: [],
    shortcuts: new Map(),
    commands: new Map(),
    contextMenuItems: new Map(),
    statusBarItems: new Map(),
    keybindings: new Map(),
    disposables: new Map(),
    registryEntries: [],
    installProgress: new Map(),
    availableUpdates: [],
    pluginLogs: new Map(),
  });
}

describe('pluginStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetPluginStore();
  });

  it('registers plugins, updates their state, and returns active plugins only', () => {
    const store = usePluginStore.getState();

    store.registerPlugin(manifest());
    store.registerPlugin(manifest({ id: 'plugin.disabled' }));
    store.setPluginState('plugin.example', 'active');
    store.setPluginState('plugin.disabled', 'disabled');

    expect(store.getPlugin('plugin.example')?.state).toBe('active');
    expect(store.getActivePlugins().map((plugin) => plugin.manifest.id)).toEqual(['plugin.example']);
  });

  it('normalizes shortcut keys and resolves handlers independent of order or case', () => {
    const handler = vi.fn();
    const store = usePluginStore.getState();

    store.registerShortcut('plugin.example', 'example.command', 'Shift+Ctrl+K', handler);

    expect(store.getShortcutHandler('ctrl+shift+k')).toBe(handler);
    expect(store.getShortcutHandler('K+SHIFT+CTRL')).toBe(handler);
  });

  it('cleanupPlugin disposes tracked resources and removes all plugin registrations', () => {
    const dispose = vi.fn();
    const store = usePluginStore.getState();

    store.registerPlugin(manifest());
    store.registerTabView('plugin.example', 'tab', (() => null) as never);
    store.registerSidebarPanel('plugin.example', 'panel', (() => null) as never, 'Panel', 'Puzzle', 'bottom');
    store.registerInputInterceptor('plugin.example', (data) => data);
    store.registerOutputProcessor('plugin.example', (data) => data);
    store.registerShortcut('plugin.example', 'command', 'ctrl+k', vi.fn());
    store.registerCommand('plugin.example', { id: 'cmd', label: 'Cmd', handler: vi.fn() });
    store.trackDisposable('plugin.example', { dispose });
    store.addPluginLog('plugin.example', 'info', 'log entry');

    store.cleanupPlugin('plugin.example');

    expect(dispose).toHaveBeenCalledTimes(1);
    expect(usePluginStore.getState().tabViews.size).toBe(0);
    expect(usePluginStore.getState().sidebarPanels.size).toBe(0);
    expect(usePluginStore.getState().inputInterceptors).toHaveLength(0);
    expect(usePluginStore.getState().outputProcessors).toHaveLength(0);
    expect(usePluginStore.getState().shortcuts.size).toBe(0);
    expect(usePluginStore.getState().commands.size).toBe(0);
    expect(usePluginStore.getState().disposables.has('plugin.example')).toBe(false);
    expect(usePluginStore.getState().pluginLogs.has('plugin.example')).toBe(false);
  });

  it('tracks install progress, available updates, and caps plugin logs at 200 entries', () => {
    const store = usePluginStore.getState();

    store.setRegistryEntries([{ id: 'plugin.example', name: 'Example', version: '1.0.0', description: '', author: '', downloads: 1, stars: 1, verified: false, tags: [] } as never]);
    store.setInstallProgress('plugin.example', 'installing');
    store.setAvailableUpdates([{ id: 'plugin.example', name: 'Example', version: '1.1.0', description: '', author: '', downloads: 1, stars: 1, verified: false, tags: [] } as never]);
    for (let index = 0; index < 205; index++) {
      store.addPluginLog('plugin.example', 'info', `log-${index}`);
    }

    expect(usePluginStore.getState().installProgress.get('plugin.example')?.state).toBe('installing');
    expect(usePluginStore.getState().hasUpdate('plugin.example')).toBe(true);
    expect(usePluginStore.getState().pluginLogs.get('plugin.example')).toHaveLength(200);
    expect(usePluginStore.getState().pluginLogs.get('plugin.example')?.[0].message).toBe('log-5');

    store.clearInstallProgress('plugin.example');
    store.clearPluginLogs('plugin.example');
    expect(usePluginStore.getState().installProgress.has('plugin.example')).toBe(false);
    expect(usePluginStore.getState().pluginLogs.has('plugin.example')).toBe(false);
  });
});