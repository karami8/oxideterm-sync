import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createMutableSelectorStore } from '@/test/helpers/mockStore';

const apiMocks = vi.hoisted(() => ({
  nodeSftpInit: vi.fn(),
  nodeSftpStat: vi.fn(),
  nodeSftpMkdir: vi.fn(),
  nodeSftpDelete: vi.fn(),
  nodeSftpDeleteRecursive: vi.fn(),
  nodeSftpRename: vi.fn(),
  nodeIdeOpenProject: vi.fn(),
  nodeIdeCheckFile: vi.fn(),
}));

const sessionTreeStoreState = vi.hoisted(() => ({
  getNode: vi.fn((nodeId: string) => ({ id: nodeId, runtime: { status: 'active' } })),
}));

const settingsStoreState = vi.hoisted(() => ({
  getIde: vi.fn(() => ({ agentMode: 'disabled' as const })),
  updateIde: vi.fn(),
}));

const agentServiceMocks = vi.hoisted(() => ({
  ensureAgent: vi.fn(),
  isAgentReady: vi.fn(),
  invalidateAgentCache: vi.fn(),
  readFile: vi.fn(),
  writeFile: vi.fn(),
}));

vi.mock('@/lib/api', () => apiMocks);
vi.mock('@/store/sessionTreeStore', () => ({
  useSessionTreeStore: createMutableSelectorStore(sessionTreeStoreState),
}));
vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: createMutableSelectorStore(settingsStoreState),
}));
vi.mock('@/lib/agentService', () => agentServiceMocks);

import { registerGitRefreshCallback, registerSearchCacheClearCallback, useIdeStore } from '@/store/ideStore';

function resetIdeState() {
  localStorage.clear();
  useIdeStore.setState({
    nodeId: null,
    terminalSessionId: null,
    project: null,
    tabs: [],
    activeTabId: null,
    treeWidth: 280,
    terminalHeight: 200,
    terminalVisible: false,
    splitDirection: null,
    splitActiveTabId: null,
    expandedPaths: new Set<string>(),
    treeRefreshSignal: {},
    conflictState: null,
    pendingScroll: null,
    cachedProjectPath: null,
    cachedTabPaths: [],
    cachedNodeId: null,
    lastClosedAt: null,
  });
}

describe('ideStore behavior', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetIdeState();
    apiMocks.nodeSftpInit.mockResolvedValue(undefined);
    apiMocks.nodeSftpStat.mockResolvedValue({
      name: 'app.ts',
      path: '/srv/app.ts',
      file_type: 'File',
      size: 10,
      modified: 100,
      permissions: '644',
    });
    apiMocks.nodeIdeOpenProject.mockResolvedValue({
      rootPath: '/srv/app',
      name: 'app',
      isGitRepo: false,
      gitBranch: null,
      fileCount: 0,
    });
    agentServiceMocks.isAgentReady.mockResolvedValue(false);
    agentServiceMocks.writeFile.mockResolvedValue({ mtime: 100 });
  });

  it('stores reconnect cache metadata when opening a project', async () => {
    await useIdeStore.getState().openProject('node-1', '~/app');

    const state = useIdeStore.getState();
    expect(apiMocks.nodeSftpInit).toHaveBeenCalledWith('node-1');
    expect(apiMocks.nodeIdeOpenProject).toHaveBeenCalledWith('node-1', '~/app');
    expect(state.project?.rootPath).toBe('/srv/app');
    expect(state.cachedProjectPath).toBe('/srv/app');
    expect(state.cachedNodeId).toBe('node-1');
    expect(state.cachedTabPaths).toEqual([]);
    expect(state.expandedPaths.has('/srv/app')).toBe(true);
  });

  it('refreshes cached project metadata when changing the root path', async () => {
    useIdeStore.setState({
      nodeId: 'node-1',
      project: { rootPath: '/srv/old', name: 'old', isGitRepo: false },
      cachedProjectPath: '/srv/old',
      cachedTabPaths: ['/srv/old/a.ts', '/srv/old/b.ts'],
      cachedNodeId: 'node-1',
      tabs: [],
    });

    apiMocks.nodeIdeOpenProject.mockResolvedValueOnce({
      rootPath: '/srv/new',
      name: 'new',
      isGitRepo: true,
      gitBranch: 'main',
      fileCount: 0,
    });

    await useIdeStore.getState().changeRootPath('/srv/new');

    const state = useIdeStore.getState();
    expect(apiMocks.nodeIdeOpenProject).toHaveBeenCalledWith('node-1', '/srv/new');
    expect(state.project).toEqual({
      rootPath: '/srv/new',
      name: 'new',
      isGitRepo: true,
      gitBranch: 'main',
    });
    expect(state.cachedProjectPath).toBe('/srv/new');
    expect(state.cachedTabPaths).toEqual([]);
    expect(state.cachedNodeId).toBe('node-1');
  });

  it('invalidates the agent cache when closing an active project', async () => {
    const clearSearchCache = vi.fn();
    registerSearchCacheClearCallback(clearSearchCache);

    await useIdeStore.getState().openProject('node-1', '~/app');

    useIdeStore.getState().closeProject(true);

    const state = useIdeStore.getState();
    expect(agentServiceMocks.invalidateAgentCache).toHaveBeenCalledWith('node-1');
    expect(clearSearchCache).toHaveBeenCalledTimes(1);
    expect(state.nodeId).toBeNull();
    expect(state.project).toBeNull();
    expect(state.cachedNodeId).toBeNull();
    expect(state.lastClosedAt).not.toBeNull();
  });

  it('reopens the same node project cleanly after a close cycle', async () => {
    await useIdeStore.getState().openProject('node-1', '~/app');
    useIdeStore.getState().closeProject(true);

    apiMocks.nodeIdeOpenProject.mockResolvedValueOnce({
      rootPath: '/srv/app',
      name: 'app',
      isGitRepo: true,
      gitBranch: 'main',
      fileCount: 0,
    });

    await useIdeStore.getState().openProject('node-1', '/srv/app');

    const state = useIdeStore.getState();
    expect(apiMocks.nodeIdeOpenProject).toHaveBeenNthCalledWith(2, 'node-1', '/srv/app');
    expect(state.nodeId).toBe('node-1');
    expect(state.project).toEqual({
      rootPath: '/srv/app',
      name: 'app',
      isGitRepo: true,
      gitBranch: 'main',
    });
    expect(state.cachedProjectPath).toBe('/srv/app');
    expect(state.cachedNodeId).toBe('node-1');
    expect(state.lastClosedAt).toBeNull();
  });

  it('detects external modifications before saving in sftp-only mode', async () => {
    useIdeStore.setState({
      nodeId: 'node-1',
      tabs: [
        {
          id: 'tab-1',
          path: '/srv/app.ts',
          name: 'app.ts',
          language: 'typescript',
          content: 'local edit',
          originalContent: 'old content',
          isDirty: true,
          isLoading: false,
          isPinned: false,
          lastAccessTime: 1,
          contentVersion: 0,
          serverMtime: 100,
        },
      ],
      conflictState: null,
    });

    apiMocks.nodeSftpStat.mockResolvedValueOnce({
      name: 'app.ts',
      path: '/srv/app.ts',
      file_type: 'File',
      size: 12,
      modified: 200,
      permissions: '644',
    });

    await expect(useIdeStore.getState().saveFile('tab-1')).rejects.toThrow('CONFLICT');

    expect(agentServiceMocks.writeFile).not.toHaveBeenCalled();
    expect(useIdeStore.getState().conflictState).toEqual({
      tabId: 'tab-1',
      localMtime: 100,
      remoteMtime: 200,
    });
  });

  it('refreshes git and search state after conflict overwrite in sftp-only mode', async () => {
    const clearSearchCache = vi.fn();
    const refreshGit = vi.fn();
    registerSearchCacheClearCallback(clearSearchCache);
    registerGitRefreshCallback(refreshGit);

    useIdeStore.setState({
      nodeId: 'node-1',
      tabs: [
        {
          id: 'tab-1',
          path: '/srv/app.ts',
          name: 'app.ts',
          language: 'typescript',
          content: 'overwrite content',
          originalContent: 'old content',
          isDirty: true,
          isLoading: false,
          isPinned: false,
          lastAccessTime: 1,
          contentVersion: 0,
          serverMtime: 100,
        },
      ],
      conflictState: {
        tabId: 'tab-1',
        localMtime: 100,
        remoteMtime: 200,
      },
    });

    agentServiceMocks.writeFile.mockResolvedValueOnce({ mtime: 300 });

    await useIdeStore.getState().resolveConflict('overwrite');

    expect(clearSearchCache).toHaveBeenCalledTimes(1);
    expect(refreshGit).toHaveBeenCalledTimes(1);
    expect(useIdeStore.getState().tabs[0]).toEqual(
      expect.objectContaining({
        isDirty: false,
        originalContent: 'overwrite content',
        serverMtime: 300,
      }),
    );
  });
});