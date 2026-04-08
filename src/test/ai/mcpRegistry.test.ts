import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const settingsStoreMock = vi.hoisted(() => ({
  state: {
    settings: {
      ai: {
        mcpServers: [
          {
            id: 'srv-1',
            name: 'server-one',
            transport: 'sse',
            url: 'http://localhost:3000/sse',
            enabled: true,
            retryOnDisconnect: true,
          },
        ],
      },
    },
  },
  store: {
    getState: () => settingsStoreMock.state,
  },
}));

const mcpClientMock = vi.hoisted(() => ({
  connectMcpServer: vi.fn(),
  disconnectMcpServer: vi.fn(),
  callMcpTool: vi.fn(),
  readMcpResource: vi.fn(),
  refreshMcpTools: vi.fn(),
}));

vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: settingsStoreMock.store,
}));

vi.mock('@/lib/ai/mcp/mcpClient', () => mcpClientMock);

describe('mcpRegistry', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.useFakeTimers();
    settingsStoreMock.state.settings.ai.mcpServers = [
      {
        id: 'srv-1',
        name: 'server-one',
        transport: 'sse',
        url: 'http://localhost:3000/sse',
        enabled: true,
        retryOnDisconnect: true,
      },
    ];
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('retries a failed SSE connection with exponential backoff', async () => {
    mcpClientMock.connectMcpServer
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'error',
        error: 'offline',
        tools: [],
        resources: [],
      })
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'connected',
        tools: [],
        resources: [],
      });

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    await useMcpRegistry.getState().connect('srv-1');

    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1000);

    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(2);
    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('connected');
  });

  it('ignores duplicate connect calls without invalidating the in-flight attempt', async () => {
    let resolveConnect: ((value: unknown) => void) | null = null;
    mcpClientMock.connectMcpServer.mockImplementation(
      () => new Promise((resolve) => {
        resolveConnect = resolve;
      }),
    );

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    const firstConnect = useMcpRegistry.getState().connect('srv-1');
    await Promise.resolve();
    await useMcpRegistry.getState().connect('srv-1');

    resolveConnect?.({
      config: settingsStoreMock.state.settings.ai.mcpServers[0],
      status: 'connected',
      runtimeId: 'runtime-1',
      tools: [],
      resources: [],
    });
    await firstConnect;

    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(1);
    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('connected');
  });

  it('cancels scheduled retry after manual disconnect', async () => {
    mcpClientMock.connectMcpServer.mockResolvedValue({
      config: settingsStoreMock.state.settings.ai.mcpServers[0],
      status: 'error',
      error: 'offline',
      tools: [],
      resources: [],
    });
    mcpClientMock.disconnectMcpServer.mockImplementation(async (state) => ({
      ...state,
      status: 'disconnected',
      runtimeId: undefined,
      tools: [],
      resources: [],
      error: undefined,
    }));

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    await useMcpRegistry.getState().connect('srv-1');
    await useMcpRegistry.getState().disconnect('srv-1');

    await vi.advanceTimersByTimeAsync(10_000);

    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(1);
    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('disconnected');
  });

  it('does not retry when the server is disabled before the timer fires', async () => {
    mcpClientMock.connectMcpServer.mockResolvedValue({
      config: settingsStoreMock.state.settings.ai.mcpServers[0],
      status: 'error',
      error: 'offline',
      tools: [],
      resources: [],
    });

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    await useMcpRegistry.getState().connect('srv-1');

    settingsStoreMock.state.settings.ai.mcpServers = [
      {
        ...settingsStoreMock.state.settings.ai.mcpServers[0],
        enabled: false,
      },
    ];

    await vi.advanceTimersByTimeAsync(1000);

    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(1);
  });

  it('ignores stale connect results after a manual disconnect invalidates the attempt', async () => {
    let resolveConnect: ((state: {
      config: { id: string; name: string; transport: string; url: string; enabled: boolean; retryOnDisconnect: boolean };
      status: 'connected';
      runtimeId: string;
      tools: never[];
      resources: never[];
    }) => void) | null = null;

    mcpClientMock.connectMcpServer.mockImplementation(
      () => new Promise((resolve) => {
        resolveConnect = resolve;
      }),
    );
    mcpClientMock.disconnectMcpServer.mockImplementation(async (state) => ({
      ...state,
      status: 'disconnected',
      runtimeId: undefined,
      tools: [],
      resources: [],
      error: undefined,
    }));

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    const connectPromise = useMcpRegistry.getState().connect('srv-1');
    await Promise.resolve();

    await useMcpRegistry.getState().disconnect('srv-1');

    resolveConnect?.({
      config: settingsStoreMock.state.settings.ai.mcpServers[0],
      status: 'connected',
      runtimeId: 'runtime-late',
      tools: [],
      resources: [],
    });
    await connectPromise;

    expect(mcpClientMock.disconnectMcpServer).toHaveBeenCalledWith(expect.objectContaining({ runtimeId: 'runtime-late' }));
    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('disconnected');
  });

  it('allows reconnect while a disconnect is still awaiting cleanup', async () => {
    let resolveDisconnect: ((value: unknown) => void) | null = null;
    mcpClientMock.disconnectMcpServer.mockImplementation(
      () => new Promise((resolve) => {
        resolveDisconnect = resolve;
      }),
    );
    mcpClientMock.connectMcpServer.mockResolvedValue({
      config: settingsStoreMock.state.settings.ai.mcpServers[0],
      status: 'connected',
      runtimeId: 'runtime-new',
      tools: [],
      resources: [],
    });

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    useMcpRegistry.setState({
      servers: new Map([[
        'srv-1',
        {
          config: settingsStoreMock.state.settings.ai.mcpServers[0],
          status: 'connected',
          runtimeId: 'runtime-old',
          tools: [],
          resources: [],
        },
      ]]),
      toolIndex: new Map(),
    });

    const disconnectPromise = useMcpRegistry.getState().disconnect('srv-1');
    await Promise.resolve();
    await useMcpRegistry.getState().connect('srv-1');

    resolveDisconnect?.({
      config: settingsStoreMock.state.settings.ai.mcpServers[0],
      status: 'disconnected',
      tools: [],
      resources: [],
    });
    await disconnectPromise;

    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(1);
    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('connected');
    expect(useMcpRegistry.getState().servers.get('srv-1')?.runtimeId).toBe('runtime-new');
  });

  it('marks a connected server as error and schedules retry after runtime failures', async () => {
    mcpClientMock.connectMcpServer
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'connected',
        runtimeId: 'runtime-1',
        capabilities: { tools: {} },
        tools: [{ name: 'ping', inputSchema: { type: 'object' } }],
        resources: [],
      })
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'connected',
        runtimeId: 'runtime-2',
        capabilities: { tools: {} },
        tools: [{ name: 'ping', inputSchema: { type: 'object' } }],
        resources: [],
      });
    mcpClientMock.callMcpTool.mockRejectedValue(new Error('socket closed'));

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    await useMcpRegistry.getState().connect('srv-1');

    await expect(useMcpRegistry.getState().callTool('srv-1', 'ping', {})).rejects.toThrow('socket closed');
    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('error');

    await vi.advanceTimersByTimeAsync(1000);

    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(2);
    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('connected');
  });

  it('closes the old stdio runtime when a connected server fails at runtime', async () => {
    settingsStoreMock.state.settings.ai.mcpServers = [
      {
        id: 'srv-stdio',
        name: 'stdio-server',
        transport: 'stdio',
        command: 'npx',
        enabled: true,
      },
    ];
    mcpClientMock.connectMcpServer.mockResolvedValue({
      config: settingsStoreMock.state.settings.ai.mcpServers[0],
      status: 'connected',
      runtimeId: 'runtime-stdio',
      capabilities: { tools: {} },
      tools: [{ name: 'ping', inputSchema: { type: 'object' } }],
      resources: [],
    });
    mcpClientMock.callMcpTool.mockRejectedValue(new Error('broken pipe'));
    mcpClientMock.disconnectMcpServer.mockImplementation(async (state) => ({
      ...state,
      status: 'disconnected',
      runtimeId: undefined,
      tools: [],
      resources: [],
      error: undefined,
    }));

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    await useMcpRegistry.getState().connect('srv-stdio');

    await expect(useMcpRegistry.getState().callTool('srv-stdio', 'ping', {})).rejects.toThrow('broken pipe');

    expect(mcpClientMock.disconnectMcpServer).toHaveBeenCalledWith(expect.objectContaining({ runtimeId: 'runtime-stdio' }));
    expect(useMcpRegistry.getState().servers.get('srv-stdio')?.status).toBe('error');
    expect(useMcpRegistry.getState().servers.get('srv-stdio')?.runtimeId).toBeUndefined();
  });

  it('ignores late runtime failures from an old connection after reconnect', async () => {
    let rejectOldCall: ((error: Error) => void) | null = null;
    mcpClientMock.connectMcpServer
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'connected',
        runtimeId: 'runtime-old',
        capabilities: { tools: {} },
        tools: [{ name: 'ping', inputSchema: { type: 'object' } }],
        resources: [],
      })
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'connected',
        runtimeId: 'runtime-new',
        capabilities: { tools: {} },
        tools: [{ name: 'ping', inputSchema: { type: 'object' } }],
        resources: [],
      });
    mcpClientMock.callMcpTool.mockImplementationOnce(
      () => new Promise((_, reject) => {
        rejectOldCall = reject;
      }),
    );
    mcpClientMock.disconnectMcpServer.mockImplementation(async (state) => ({
      ...state,
      status: 'disconnected',
      runtimeId: undefined,
      tools: [],
      resources: [],
      error: undefined,
    }));

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    await useMcpRegistry.getState().connect('srv-1');

    const oldCall = useMcpRegistry.getState().callTool('srv-1', 'ping', {});
    await Promise.resolve();
    await useMcpRegistry.getState().disconnect('srv-1');
    await useMcpRegistry.getState().connect('srv-1');

    rejectOldCall?.(new Error('old socket closed'));
    await expect(oldCall).rejects.toThrow('old socket closed');

    expect(useMcpRegistry.getState().servers.get('srv-1')?.status).toBe('connected');
    expect(useMcpRegistry.getState().servers.get('srv-1')?.runtimeId).toBe('runtime-new');
    expect(mcpClientMock.connectMcpServer).toHaveBeenCalledTimes(2);
  });

  it('does not let a stale refreshTools result roll back a newer connection state', async () => {
    let resolveRefresh: ((tools: Array<{ name: string; inputSchema: { type: 'object' } }>) => void) | null = null;
    mcpClientMock.connectMcpServer
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'connected',
        runtimeId: 'runtime-old',
        capabilities: { tools: {} },
        tools: [{ name: 'old-tool', inputSchema: { type: 'object' } }],
        resources: [],
      })
      .mockResolvedValueOnce({
        config: settingsStoreMock.state.settings.ai.mcpServers[0],
        status: 'connected',
        runtimeId: 'runtime-new',
        capabilities: { tools: {} },
        tools: [{ name: 'new-tool', inputSchema: { type: 'object' } }],
        resources: [],
      });
    mcpClientMock.refreshMcpTools.mockImplementationOnce(
      () => new Promise((resolve) => {
        resolveRefresh = resolve;
      }),
    );
    mcpClientMock.disconnectMcpServer.mockImplementation(async (state) => ({
      ...state,
      status: 'disconnected',
      runtimeId: undefined,
      tools: [],
      resources: [],
      error: undefined,
    }));

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    await useMcpRegistry.getState().connect('srv-1');

    const refreshPromise = useMcpRegistry.getState().refreshTools('srv-1');
    await Promise.resolve();
    await useMcpRegistry.getState().disconnect('srv-1');
    await useMcpRegistry.getState().connect('srv-1');

    resolveRefresh?.([{ name: 'stale-tool', inputSchema: { type: 'object' } }]);
    await refreshPromise;

    const server = useMcpRegistry.getState().servers.get('srv-1');
    expect(server?.status).toBe('connected');
    expect(server?.runtimeId).toBe('runtime-new');
    expect(server?.tools).toEqual([{ name: 'new-tool', inputSchema: { type: 'object' } }]);
  });

  it('disambiguates tool names when two connected servers share the same display name', async () => {
    settingsStoreMock.state.settings.ai.mcpServers = [
      {
        id: 'srv-a',
        name: 'shared-name',
        transport: 'sse',
        url: 'http://localhost:3000/a',
        enabled: true,
      },
      {
        id: 'srv-b',
        name: 'shared-name',
        transport: 'sse',
        url: 'http://localhost:3000/b',
        enabled: true,
      },
    ];

    const { useMcpRegistry } = await import('@/lib/ai/mcp/mcpRegistry');
    useMcpRegistry.setState({
      servers: new Map([
        ['srv-a', {
          config: settingsStoreMock.state.settings.ai.mcpServers[0],
          status: 'connected',
          tools: [{ name: 'ping', inputSchema: { type: 'object' } }],
          resources: [],
        }],
        ['srv-b', {
          config: settingsStoreMock.state.settings.ai.mcpServers[1],
          status: 'connected',
          tools: [{ name: 'ping', inputSchema: { type: 'object' } }],
          resources: [],
        }],
      ]),
      toolIndex: new Map(),
    });

    const names = useMcpRegistry.getState().getAllMcpToolDefinitions().map((tool) => tool.name).sort();

    expect(names).toEqual([
      'mcp::shared-name#srv-a::ping',
      'mcp::shared-name#srv-b::ping',
    ]);
  });
});