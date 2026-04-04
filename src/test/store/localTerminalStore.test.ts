import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  localListShells: vi.fn(),
  localGetDefaultShell: vi.fn(),
  localCreateTerminal: vi.fn(),
  localCloseTerminal: vi.fn(),
  localResizeTerminal: vi.fn(),
  localWriteTerminal: vi.fn(),
  localListTerminals: vi.fn(),
  localCleanupDeadSessions: vi.fn(),
  localDetachTerminal: vi.fn(),
  localAttachTerminal: vi.fn(),
  localListBackground: vi.fn(),
  localCheckChildProcesses: vi.fn(),
}));

const toastMock = vi.hoisted(() => ({ addToast: vi.fn() }));

const settingsStoreMock = vi.hoisted(() => ({
  state: {
    settings: {
      localTerminal: {
        defaultShellId: 'zsh',
        recentShellIds: [],
        defaultCwd: '/workspace',
        loadShellProfile: true,
        ohMyPoshEnabled: false,
        ohMyPoshTheme: null,
        customEnvVars: {},
      },
    },
  },
  store: {
    getState: () => settingsStoreMock.state,
  },
}));

vi.mock('@/lib/api', () => ({ api: apiMocks }));

vi.mock('@/hooks/useToast', () => ({
  useToastStore: {
    getState: () => toastMock,
  },
}));

vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: settingsStoreMock.store,
}));

vi.mock('@/i18n', () => ({
  default: {
    t: (key: string) => key,
  },
}));

import { useLocalTerminalStore } from '@/store/localTerminalStore';
import type { BackgroundSessionInfo, LocalTerminalInfo, ShellInfo } from '@/types';

function makeShell(overrides: Partial<ShellInfo> = {}): ShellInfo {
  return {
    id: 'zsh',
    label: 'Zsh',
    path: '/bin/zsh',
    args: ['--login'],
    ...overrides,
  };
}

function makeTerminal(overrides: Partial<LocalTerminalInfo> = {}): LocalTerminalInfo {
  return {
    id: 'term-1',
    shell: makeShell(),
    cols: 80,
    rows: 24,
    running: true,
    detached: false,
    ...overrides,
  };
}

function makeBackgroundSession(overrides: Partial<BackgroundSessionInfo> = {}): BackgroundSessionInfo {
  return {
    id: 'bg-1',
    shell: makeShell(),
    cols: 100,
    rows: 40,
    running: true,
    detachedSecs: 12,
    bufferLines: 50,
    ...overrides,
  };
}

function resetLocalTerminalStore() {
  useLocalTerminalStore.setState({
    terminals: new Map(),
    shells: [],
    defaultShell: null,
    shellsLoaded: false,
    backgroundSessions: new Map(),
    pendingReplay: new Map(),
  });
}

describe('localTerminalStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetLocalTerminalStore();
    settingsStoreMock.state.settings.localTerminal.defaultShellId = 'zsh';
    settingsStoreMock.state.settings.localTerminal.defaultCwd = '/workspace';
  });

  it('loads shells on demand and uses configured default shell when creating terminals', async () => {
    apiMocks.localListShells.mockResolvedValue([makeShell(), makeShell({ id: 'bash', path: '/bin/bash' })]);
    apiMocks.localGetDefaultShell.mockResolvedValue(makeShell());
    apiMocks.localCreateTerminal.mockResolvedValue({
      sessionId: 'term-1',
      info: makeTerminal(),
    });

    const terminal = await useLocalTerminalStore.getState().createTerminal();

    expect(apiMocks.localListShells).toHaveBeenCalledTimes(1);
    expect(apiMocks.localCreateTerminal).toHaveBeenCalledWith(
      expect.objectContaining({
        shellPath: '/bin/zsh',
        cwd: '/workspace',
        loadProfile: true,
      }),
    );
    expect(terminal.id).toBe('term-1');
    expect(useLocalTerminalStore.getState().terminals.get('term-1')?.shell.path).toBe('/bin/zsh');
  });

  it('ignores invalid resize requests and updates local terminal dimensions on valid resize', async () => {
    useLocalTerminalStore.setState({ terminals: new Map([['term-1', makeTerminal()]]) });

    await useLocalTerminalStore.getState().resizeTerminal('term-1', 0, 24);
    expect(apiMocks.localResizeTerminal).not.toHaveBeenCalled();

    await useLocalTerminalStore.getState().resizeTerminal('term-1', 120, 50);
    expect(apiMocks.localResizeTerminal).toHaveBeenCalledWith('term-1', 120, 50);
    expect(useLocalTerminalStore.getState().terminals.get('term-1')).toMatchObject({ cols: 120, rows: 50 });
  });

  it('attaches background terminals, stores replay, and consumeReplay is one-shot', async () => {
    useLocalTerminalStore.setState({
      backgroundSessions: new Map([['bg-1', makeBackgroundSession()]]),
    });
    apiMocks.localAttachTerminal.mockResolvedValue([1, 2, 3]);

    const replay = await useLocalTerminalStore.getState().attachTerminal('bg-1');

    expect(replay).toEqual([1, 2, 3]);
    expect(useLocalTerminalStore.getState().backgroundSessions.has('bg-1')).toBe(false);
    expect(useLocalTerminalStore.getState().terminals.get('bg-1')).toMatchObject({
      id: 'bg-1',
      cols: 100,
      rows: 40,
      detached: false,
    });
    expect(useLocalTerminalStore.getState().consumeReplay('bg-1')).toEqual([1, 2, 3]);
    expect(useLocalTerminalStore.getState().consumeReplay('bg-1')).toBeUndefined();
  });

  it('cleanupDeadSessions removes stale terminal entries and returns removed ids', async () => {
    useLocalTerminalStore.setState({
      terminals: new Map([
        ['term-1', makeTerminal()],
        ['term-2', makeTerminal({ id: 'term-2' })],
      ]),
    });
    apiMocks.localCleanupDeadSessions.mockResolvedValue(['term-2']);

    const removed = await useLocalTerminalStore.getState().cleanupDeadSessions();

    expect(removed).toEqual(['term-2']);
    expect(useLocalTerminalStore.getState().terminals.has('term-1')).toBe(true);
    expect(useLocalTerminalStore.getState().terminals.has('term-2')).toBe(false);
  });

  it('removes terminal locally even when backend close fails', async () => {
    useLocalTerminalStore.setState({
      terminals: new Map([['term-1', makeTerminal()]]),
      backgroundSessions: new Map([['term-1', makeBackgroundSession({ id: 'term-1' })]]),
      pendingReplay: new Map([['term-1', [9, 9, 9]]]),
    });
    apiMocks.localCloseTerminal.mockRejectedValue(new Error('backend unavailable'));

    await useLocalTerminalStore.getState().closeTerminal('term-1');

    expect(useLocalTerminalStore.getState().terminals.has('term-1')).toBe(false);
    expect(useLocalTerminalStore.getState().backgroundSessions.has('term-1')).toBe(false);
    expect(useLocalTerminalStore.getState().pendingReplay.has('term-1')).toBe(false);
  });
});