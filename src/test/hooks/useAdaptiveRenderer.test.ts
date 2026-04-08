import { renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { findCursorControlBoundary, useAdaptiveRenderer } from '@/hooks/useAdaptiveRenderer';
import { adaptiveRendererIssue26Fixtures } from '@/test/fixtures/adaptiveRendererIssue26Fixtures';

function textEncoder(input: string): Uint8Array {
  return new TextEncoder().encode(input);
}

function createRendererHarness() {
  const writes: string[] = [];
  const terminal = {
    write: vi.fn((data: Uint8Array, callback?: () => void) => {
      writes.push(new TextDecoder().decode(data));
      callback?.();
    }),
  };

  let rafCallback: FrameRequestCallback | null = null;
  vi.stubGlobal('requestAnimationFrame', vi.fn((cb: FrameRequestCallback) => {
    rafCallback = cb;
    return 1;
  }));
  vi.stubGlobal('cancelAnimationFrame', vi.fn());

  const terminalRef = { current: terminal as never };
  const hook = renderHook(() => useAdaptiveRenderer({ terminalRef, mode: 'auto' }));

  return {
    writes,
    scheduleWrite: hook.result.current.scheduleWrite,
    flushRaf: () => rafCallback?.(16.7),
    hasPendingRaf: () => rafCallback !== null,
  };
}

describe('findCursorControlBoundary', () => {
  it('detects destructive CSI sequences at the start of a chunk', () => {
    expect(findCursorControlBoundary(textEncoder('\x1b[2Kprompt'))).toBe(0);
  });

  it('detects destructive CSI sequences after printable output', () => {
    expect(findCursorControlBoundary(textEncoder('file1\r\nfile2\r\n\x1b[2A\x1b[2K'))).toBe(14);
  });

  it('skips non-destructive CSI sequences and finds a later destructive one', () => {
    expect(findCursorControlBoundary(textEncoder('\x1b[31mred\x1b[0mfile\r\n\x1b[2Kprompt'))).toBe(18);
  });

  it('ignores non-destructive CSI sequences such as SGR color changes', () => {
    expect(findCursorControlBoundary(textEncoder('\x1b[31mred\x1b[0m'))).toBe(-1);
  });
});

describe('useAdaptiveRenderer', () => {
  it('flushes printable output before a later destructive cursor-control tail', () => {
    const { writes, scheduleWrite, flushRaf, hasPendingRaf } = createRendererHarness();

    scheduleWrite(textEncoder('file1\r\nfile2\r\n\x1b[2A\x1b[2Kprompt$ '));

    expect(writes).toEqual(['file1\r\nfile2\r\n']);
    expect(hasPendingRaf()).toBe(true);

    flushRaf();

    expect(writes).toEqual([
      'file1\r\nfile2\r\n',
      '\x1b[2A\x1b[2Kprompt$ ',
    ]);
  });

  it('keeps inline redraw sequences in a single write when there is no prior line output', () => {
    const { writes, scheduleWrite, flushRaf, hasPendingRaf } = createRendererHarness();

    scheduleWrite(textEncoder('hello\x1b[1Gworld'));

    expect(writes).toEqual([]);
    expect(hasPendingRaf()).toBe(true);

    flushRaf();

    expect(writes).toEqual(['hello\x1b[1Gworld']);
  });

  it('keeps carriage-return-based single-line redraw in a single write', () => {
    const { writes, scheduleWrite, flushRaf, hasPendingRaf } = createRendererHarness();

    scheduleWrite(textEncoder('42%\r\x1b[2K43%'));

    expect(writes).toEqual([]);
    expect(hasPendingRaf()).toBe(true);

    flushRaf();

    expect(writes).toEqual(['42%\r\x1b[2K43%']);
  });

  it('flushes a pending single-line chunk before a later redraw chunk arrives', () => {
    const { writes, scheduleWrite, flushRaf, hasPendingRaf } = createRendererHarness();

    scheduleWrite(textEncoder('hello'));
    scheduleWrite(textEncoder('\x1b[1Gworld'));

    expect(writes).toEqual(['hello']);
    expect(hasPendingRaf()).toBe(true);

    flushRaf();

    expect(writes).toEqual(['hello', '\x1b[1Gworld']);
  });

  describe('Issue #26 async prompt redraw regression', () => {
    it.each(adaptiveRendererIssue26Fixtures)('$name', (fixture) => {
      const { writes, scheduleWrite, flushRaf, hasPendingRaf } = createRendererHarness();

      fixture.chunks.forEach((chunk, index) => {
        scheduleWrite(textEncoder(chunk));
        expect(writes).toEqual(fixture.writesAfterChunk[index]);
        expect(hasPendingRaf()).toBe(true);
      });

      flushRaf();

      expect(writes).toEqual(fixture.finalWrites);
    });

  });
});