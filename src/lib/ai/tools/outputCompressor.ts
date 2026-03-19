/**
 * Output Compressor — Pre-processing pipeline for tool output
 *
 * Applied before truncation to maximize information density within the
 * MAX_OUTPUT_BYTES budget. Each stage is cheap (single-pass regex/iteration)
 * so there's no measurable latency impact on tool execution.
 */

// ═══════════════════════════════════════════════════════════════════════════
// ANSI Escape Code Stripping
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Strip ANSI/VT escape sequences (colors, cursor moves, etc.).
 * These are meaningless to LLMs and waste ~5-15% of tokens on colored output.
 */
// eslint-disable-next-line no-control-regex
const ANSI_RE = /[\x1b\x9b][[\]()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><~]/g;

export function stripAnsi(text: string): string {
  return text.replace(ANSI_RE, '');
}

// ═══════════════════════════════════════════════════════════════════════════
// Blank Line Collapsing
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Collapse runs of 2+ blank lines into a single blank line.
 */
export function collapseBlankLines(text: string): string {
  return text.replace(/\n{3,}/g, '\n\n');
}

// ═══════════════════════════════════════════════════════════════════════════
// Duplicate Line Folding
// ═══════════════════════════════════════════════════════════════════════════

/** Minimum consecutive duplicates before folding */
const MIN_DUP_RUN = 3;

/**
 * Fold runs of ≥3 identical consecutive lines into a summary.
 * Common in log tails, test output, and progress bars.
 *
 * Example: 10 identical "Processing..." lines → first line + "(... repeated ×9)"
 */
export function foldDuplicateLines(text: string): string {
  const lines = text.split('\n');
  if (lines.length < MIN_DUP_RUN) return text;

  const result: string[] = [];
  let i = 0;

  while (i < lines.length) {
    const current = lines[i];
    let runLength = 1;

    // Count consecutive identical lines
    while (i + runLength < lines.length && lines[i + runLength] === current) {
      runLength++;
    }

    if (runLength >= MIN_DUP_RUN) {
      result.push(current);
      result.push(`(... repeated ×${runLength - 1})`);
    } else {
      for (let j = 0; j < runLength; j++) {
        result.push(lines[i + j]);
      }
    }

    i += runLength;
  }

  return result.join('\n');
}

// ═══════════════════════════════════════════════════════════════════════════
// Combined Pipeline
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Full compression pipeline: ANSI strip → blank line collapse → duplicate fold.
 * Called by `truncateOutput()` before the byte-limit truncation.
 */
export function compressOutput(text: string): string {
  let out = stripAnsi(text);
  out = collapseBlankLines(out);
  out = foldDuplicateLines(out);
  return out;
}
