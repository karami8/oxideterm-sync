import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Terminal } from '@xterm/xterm';

async function flushMicrotasks() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

const osc52State = vi.hoisted(() => ({ enabled: true }));
const nativeClipboardState = vi.hoisted(() => ({
  writeText: vi.fn(),
  readText: vi.fn(),
}));

vi.mock('@/store/settingsStore', () => ({
  useSettingsStore: {
    getState: vi.fn(() => ({
      settings: {
        terminal: {
          osc52Clipboard: osc52State.enabled,
        },
      },
    })),
  },
}));

type OscHandler = (data: string) => boolean;
type ClipboardProvider = {
  readText: (selection: string) => Promise<string>;
  writeText: (selection: string, text: string) => Promise<void>;
};
type Base64Codec = {
  encodeText: (data: string) => string;
  decodeText: (data: string) => string;
};
type ClipboardAddonModule = {
  ClipboardAddon?: new (base64: Base64Codec, provider: ClipboardProvider) => {
    activate: () => void;
    dispose: () => void;
    base64?: Base64Codec;
    provider?: ClipboardProvider;
  };
};

function createTerminalMock() {
  let oscHandler: OscHandler | null = null;

  return {
    term: {
      loadAddon: vi.fn(),
      parser: {
        registerOscHandler: vi.fn((_code: number, handler: OscHandler) => {
          oscHandler = handler;
          return { dispose: vi.fn() };
        }),
      },
    } as unknown as Terminal,
    getOscHandler: () => oscHandler,
  };
}

async function importClipboardSupportWithAddon(mockFactory: () => ClipboardAddonModule) {
  vi.resetModules();
  vi.doMock('@tauri-apps/plugin-clipboard-manager', () => ({
    writeText: nativeClipboardState.writeText,
    readText: nativeClipboardState.readText,
  }));
  vi.doMock('@xterm/addon-clipboard', mockFactory);
  return import('@/lib/clipboardSupport');
}

describe('installTerminalClipboardSupport', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    osc52State.enabled = true;
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
    nativeClipboardState.writeText.mockReset();
    nativeClipboardState.readText.mockReset();
    nativeClipboardState.writeText.mockResolvedValue(undefined);
    nativeClipboardState.readText.mockResolvedValue('native clipboard text');
  });

  it('uses the clipboard addon path to write OSC52 clipboard data when enabled', async () => {
    const addonInstances: Array<{
      activate: ReturnType<typeof vi.fn>;
      dispose: ReturnType<typeof vi.fn>;
      base64: Base64Codec;
      provider: ClipboardProvider;
    }> = [];
    class MockClipboardAddon {
      activate = vi.fn();
      dispose = vi.fn();

      constructor(
        public base64: Base64Codec,
        public provider: ClipboardProvider,
      ) {
        addonInstances.push(this);
      }
    }
    const { term } = createTerminalMock();
    const { installTerminalClipboardSupport } = await importClipboardSupportWithAddon(() => ({
      ClipboardAddon: MockClipboardAddon,
    }));

    await installTerminalClipboardSupport(term);

    expect(term.loadAddon).toHaveBeenCalledTimes(1);
    const [{ base64, provider }] = addonInstances;

    await provider.writeText('c', 'hello from nvim');

    expect(nativeClipboardState.writeText).toHaveBeenCalledWith('hello from nvim');
    await expect(provider.readText('c')).resolves.toBe('');
    expect(base64.decodeText(base64.encodeText('汉字 clipboard'))).toBe('汉字 clipboard');
  });

  it('does not write through the addon provider when osc52 is disabled', async () => {
    const addonInstances: Array<{ provider: ClipboardProvider }> = [];
    class MockClipboardAddon {
      activate = vi.fn();
      dispose = vi.fn();

      constructor(
        _base64: Base64Codec,
        public provider: ClipboardProvider,
      ) {
        addonInstances.push(this);
      }
    }
    const { term } = createTerminalMock();
    const { installTerminalClipboardSupport } = await importClipboardSupportWithAddon(() => ({
      ClipboardAddon: MockClipboardAddon,
    }));

    await installTerminalClipboardSupport(term);
    osc52State.enabled = false;

    await addonInstances[0].provider.writeText('c', 'blocked');

    expect(nativeClipboardState.writeText).not.toHaveBeenCalled();
  });

  it('falls back to a parser OSC52 handler when the clipboard addon is unavailable', async () => {
    const { term, getOscHandler } = createTerminalMock();
    const { installTerminalClipboardSupport } = await importClipboardSupportWithAddon(() => ({}));

    await installTerminalClipboardSupport(term);

    const handled = getOscHandler()?.(`c;${btoa('copied from osc52')}`);
    await flushMicrotasks();

    expect(handled).toBe(true);
    expect(nativeClipboardState.writeText).toHaveBeenCalledWith('copied from osc52');
  });

  it('stops processing fallback OSC52 writes immediately after the setting is disabled', async () => {
    const { term, getOscHandler } = createTerminalMock();
    const { installTerminalClipboardSupport } = await importClipboardSupportWithAddon(() => ({}));

    await installTerminalClipboardSupport(term);
    osc52State.enabled = false;

    expect(getOscHandler()?.(`c;${btoa('should not copy')}`)).toBe(true);
    expect(nativeClipboardState.writeText).not.toHaveBeenCalled();
  });

  it('ignores non-clipboard selectors, read requests, invalid payloads, and oversized payloads in fallback mode', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const { term, getOscHandler } = createTerminalMock();
    const { installTerminalClipboardSupport } = await importClipboardSupportWithAddon(() => ({}));

    await installTerminalClipboardSupport(term);
    const handler = getOscHandler();

    expect(handler?.('p;Zm9v')).toBe(true);
    expect(handler?.('c;?')).toBe(true);
    expect(handler?.('c;***not-base64***')).toBe(true);
    expect(handler?.(`c;${'A'.repeat(1_048_577)}`)).toBe(true);

    expect(nativeClipboardState.writeText).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalledWith('[OSC 52] Invalid base64 payload');
    expect(warn).toHaveBeenCalledWith('[OSC 52] Payload too large, ignored');
  });

  it('warns clearly when fallback mode receives OSC52 data but the browser clipboard API is unavailable', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const { term, getOscHandler } = createTerminalMock();
    const { installTerminalClipboardSupport } = await importClipboardSupportWithAddon(() => ({}));

    nativeClipboardState.writeText.mockRejectedValueOnce(new Error('native clipboard unavailable'));

    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: undefined,
    });

    await installTerminalClipboardSupport(term);

    expect(getOscHandler()?.(`c;${btoa('text')}`)).toBe(true);
    await flushMicrotasks();
    expect(warn).toHaveBeenCalledWith('[OSC 52] Clipboard write is unavailable in this environment');
  });

  it('reads clipboard text through the native Tauri plugin first', async () => {
    const { readSystemClipboardText } = await importClipboardSupportWithAddon(() => ({}));

    await expect(readSystemClipboardText()).resolves.toBe('native clipboard text');
    expect(nativeClipboardState.readText).toHaveBeenCalledOnce();
  });

  it('falls back to navigator.clipboard.readText when the native clipboard plugin read fails', async () => {
    const browserReadText = vi.fn().mockResolvedValue('browser clipboard text');
    const { readSystemClipboardText } = await importClipboardSupportWithAddon(() => ({}));

    nativeClipboardState.readText.mockRejectedValueOnce(new Error('native read unavailable'));
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: vi.fn().mockResolvedValue(undefined),
        readText: browserReadText,
      },
    });

    await expect(readSystemClipboardText()).resolves.toBe('browser clipboard text');
    expect(browserReadText).toHaveBeenCalledOnce();
  });
});