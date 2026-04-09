import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

async function flushAsyncWork() {
  await Promise.resolve();
  await Promise.resolve();
}

const nodeAgentStatusMock = vi.hoisted(() => vi.fn());

vi.mock('@/lib/api', () => ({
  nodeAgentStatus: nodeAgentStatusMock,
}));

import { useAgentStatus } from '@/components/ide/hooks/useAgentStatus';

describe('useAgentStatus', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('resets stale status when nodeId becomes undefined', async () => {
    nodeAgentStatusMock.mockResolvedValueOnce({ type: 'ready', version: '1.0.0', arch: 'x86_64', pid: 1 });

    const { result, rerender } = renderHook(({ nodeId }) => useAgentStatus(nodeId), {
      initialProps: { nodeId: 'node-1' as string | undefined },
    });

    await waitFor(() => expect(result.current.mode).toBe('agent'));
    rerender({ nodeId: undefined });

    expect(result.current.mode).toBe('checking');
    expect(result.current.status).toBeNull();
    expect(result.current.label).toBe('…');
  });

  it('switches back to checking immediately when the node changes before the next status resolves', async () => {
    let resolveNode2: ((value: { type: 'notDeployed' }) => void) | undefined;
    nodeAgentStatusMock
      .mockResolvedValueOnce({ type: 'ready', version: '1.0.0', arch: 'x86_64', pid: 1 })
      .mockImplementationOnce(() => new Promise<{ type: 'notDeployed' }>((resolve) => {
        resolveNode2 = resolve;
      }));

    const { result, rerender } = renderHook(({ nodeId }) => useAgentStatus(nodeId), {
      initialProps: { nodeId: 'node-1' as string | undefined },
    });

    await waitFor(() => expect(result.current.mode).toBe('agent'));

    rerender({ nodeId: 'node-2' });

    expect(result.current.mode).toBe('checking');
    expect(result.current.status).toBeNull();

    if (!resolveNode2) {
      throw new Error('Expected deferred status resolver to be initialized');
    }

    resolveNode2({ type: 'notDeployed' });

    await waitFor(() => expect(result.current.mode).toBe('sftp'));
    expect(result.current.status).toEqual({ type: 'notDeployed' });
  });

  it('detects agent loss while already in agent mode via periodic polling', async () => {
    vi.useFakeTimers();
    nodeAgentStatusMock
      .mockResolvedValueOnce({ type: 'ready', version: '1.0.0', arch: 'x86_64', pid: 1 })
      .mockResolvedValueOnce({ type: 'failed', reason: 'channel closed' });

    const { result, unmount } = renderHook(() => useAgentStatus('node-1'));

    await act(async () => {
      await flushAsyncWork();
    });

    expect(result.current.mode).toBe('agent');

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
      await flushAsyncWork();
    });

    expect(result.current.mode).toBe('sftp');
    expect(result.current.status).toEqual({ type: 'failed', reason: 'channel closed' });

    unmount();
    vi.useRealTimers();
  }, 10000);
});