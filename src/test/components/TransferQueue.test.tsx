import { render, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createMutableSelectorStore } from '@/test/helpers/mockStore';
import type { NodeStateSnapshot } from '@/types';

const apiMocks = vi.hoisted(() => ({
  nodeSftpListIncompleteTransfers: vi.fn(),
  nodeSftpResumeTransfer: vi.fn(),
}));

const nodeStateMock = vi.hoisted(() => ({
  value: {
    state: { readiness: 'ready' as const, sftpReady: true },
    ready: true,
    generation: 1,
  } as { state: NodeStateSnapshot['state']; ready: boolean; generation: number },
}));

const transferStoreState = vi.hoisted(() => ({
  getAllTransfers: vi.fn(() => []),
  clearCompleted: vi.fn(),
  cancelTransfer: vi.fn(),
  removeTransfer: vi.fn(),
  addTransfer: vi.fn(),
  pauseTransfer: vi.fn(),
  resumeTransfer: vi.fn(),
}));

vi.mock('@/lib/api', () => ({
  nodeSftpListIncompleteTransfers: apiMocks.nodeSftpListIncompleteTransfers,
  nodeSftpResumeTransfer: apiMocks.nodeSftpResumeTransfer,
}));

vi.mock('@/hooks/useNodeState', () => ({
  useNodeState: () => nodeStateMock.value,
}));

vi.mock('@/store/transferStore', async () => {
  const actual = await vi.importActual<typeof import('@/store/transferStore')>('@/store/transferStore');
  return {
    ...actual,
    useTransferStore: createMutableSelectorStore(transferStoreState),
  };
});

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>();
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, options?: Record<string, unknown>) => {
        if (key === 'sftp.queue.incomplete_count') {
          return `incomplete ${String(options?.count ?? 0)}`;
        }
        if (key === 'sftp.queue.active_count') {
          return `active ${String(options?.count ?? 0)}`;
        }
        return key;
      },
    }),
  };
});

import { TransferQueue, createResumedTransferSeed } from '@/components/sftp/TransferQueue';
import type { IncompleteTransferInfo } from '@/types';

function makeIncompleteTransfer(
  overrides: Partial<IncompleteTransferInfo> = {},
): IncompleteTransferInfo {
  return {
    transfer_id: 'transfer-1',
    transfer_type: 'Download',
    source_path: '/remote/file.txt',
    destination_path: '/local/file.txt',
    transferred_bytes: 5,
    total_bytes: 10,
    status: 'Failed',
    session_id: 'conn-legacy',
    error: 'boom',
    progress_percent: 50,
    can_resume: true,
    ...overrides,
  };
}

describe('TransferQueue', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    nodeStateMock.value = {
      state: { readiness: 'ready', sftpReady: true },
      ready: true,
      generation: 1,
    };
    apiMocks.nodeSftpListIncompleteTransfers.mockResolvedValue([]);
    apiMocks.nodeSftpResumeTransfer.mockResolvedValue(undefined);
    transferStoreState.getAllTransfers.mockReturnValue([]);
  });

  it('creates resumed transfer seeds with the active node ID instead of the stored connection ID', () => {
    const seed = createResumedTransferSeed(
      'node-active',
      makeIncompleteTransfer({
        session_id: 'conn-stale',
        transfer_type: 'Upload',
        source_path: '/local/example.txt',
        destination_path: '/remote/example.txt',
      }),
    );

    expect(seed).toEqual({
      id: 'transfer-1',
      nodeId: 'node-active',
      name: 'example.txt',
      localPath: '/local/example.txt',
      remotePath: '/remote/example.txt',
      direction: 'upload',
      size: 10,
    });
  });

  it('loads incomplete transfers only after the node becomes ready', async () => {
    render(<TransferQueue nodeId="node-1" />);

    await waitFor(() => {
      expect(apiMocks.nodeSftpListIncompleteTransfers).toHaveBeenCalledWith('node-1');
    });
  });

  it('skips incomplete transfer loading while the node is not ready', () => {
    nodeStateMock.value = {
      state: { readiness: 'connecting', sftpReady: false },
      ready: true,
      generation: 1,
    };

    render(<TransferQueue nodeId="node-1" />);

    expect(apiMocks.nodeSftpListIncompleteTransfers).not.toHaveBeenCalled();
  });
});