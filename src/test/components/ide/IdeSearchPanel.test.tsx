import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createMutableSelectorStore } from '@/test/helpers/mockStore';

const ideStoreState = vi.hoisted(() => ({
  nodeId: 'node-1' as string | null,
  activeTabId: 'tab-1' as string | null,
  project: {
    rootPath: '/srv/app',
    name: 'app',
    isGitRepo: true,
  },
  openFile: vi.fn(async () => undefined),
  setPendingScroll: vi.fn(),
}));

const agentServiceMocks = vi.hoisted(() => ({
  grep: vi.fn(),
}));

vi.mock('@/store/ideStore', () => {
  const useIdeStore = createMutableSelectorStore(ideStoreState);
  return {
    useIdeStore,
    useIdeProject: () => ideStoreState.project,
    registerSearchCacheClearCallback: vi.fn(),
  };
});

vi.mock('@/lib/agentService', () => agentServiceMocks);

vi.mock('@/lib/api', () => ({
  nodeIdeExecCommand: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string | Record<string, unknown>) =>
      typeof fallback === 'string' ? fallback : key,
  }),
}));

import { IdeSearchPanel, clearSearchCache } from '@/components/ide/IdeSearchPanel';

describe('IdeSearchPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearSearchCache();
    ideStoreState.nodeId = 'node-1';
    ideStoreState.activeTabId = 'tab-1';
    ideStoreState.project = {
      rootPath: '/srv/app',
      name: 'app',
      isGitRepo: true,
    };
  });

  it('opens absolute agent grep results without prefixing the project root again', async () => {
    agentServiceMocks.grep.mockResolvedValue([
      {
        path: '/srv/app/src/main.ts',
        line: 7,
        column: 6,
        text: 'const needle = true;',
      },
    ]);

    render(<IdeSearchPanel open onClose={vi.fn()} />);

    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'needle' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    const hit = await screen.findByText('needle');
    fireEvent.click(hit);

    await waitFor(() => {
      expect(ideStoreState.openFile).toHaveBeenCalledWith('/srv/app/src/main.ts');
    });
    expect(ideStoreState.setPendingScroll).toHaveBeenCalledWith('tab-1', 7, 6);
  });

  it('still resolves relative grep results against the project root', async () => {
    agentServiceMocks.grep.mockResolvedValue([
      {
        path: 'src/relative.ts',
        line: 3,
        column: 2,
        text: 'relative needle match',
      },
    ]);

    render(<IdeSearchPanel open onClose={vi.fn()} />);

    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'needle' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    const hit = await screen.findByText('needle');
    fireEvent.click(hit);

    await waitFor(() => {
      expect(ideStoreState.openFile).toHaveBeenCalledWith('/srv/app/src/relative.ts');
    });
    expect(ideStoreState.setPendingScroll).toHaveBeenCalledWith('tab-1', 3, 2);
  });
});