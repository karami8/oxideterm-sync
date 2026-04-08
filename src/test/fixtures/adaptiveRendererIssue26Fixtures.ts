export type AdaptiveRendererIssue26Fixture = {
  name: string;
  chunks: string[];
  writesAfterChunk: string[][];
  finalWrites: string[];
};

const multilinePrefix = 'file1\r\nfile2\r\n';
const coloredMultilinePrefix = '\x1b[32mfile1\x1b[0m\r\n\x1b[36mfile2\x1b[0m\r\n';
const redrawTail = '\x1b[2A\x1b[2Kprompt$ ';

export const adaptiveRendererIssue26Fixtures: AdaptiveRendererIssue26Fixture[] = [
  {
    name: 'same chunk with colored multiline output and redraw tail',
    chunks: [coloredMultilinePrefix + redrawTail],
    writesAfterChunk: [[coloredMultilinePrefix]],
    finalWrites: [coloredMultilinePrefix, redrawTail],
  },
  {
    name: 'destructive redraw split after ESC[ prefix',
    chunks: [multilinePrefix + '\x1b[', '2A\x1b[2Kprompt$ '],
    writesAfterChunk: [[], [multilinePrefix]],
    finalWrites: [multilinePrefix, redrawTail],
  },
  {
    name: 'destructive redraw split after bare ESC byte',
    chunks: [multilinePrefix + '\x1b', '[2A\x1b[2Kprompt$ '],
    writesAfterChunk: [[], [multilinePrefix]],
    finalWrites: [multilinePrefix, redrawTail],
  },
  {
    name: 'destructive redraw split mid-parameter sequence',
    chunks: [multilinePrefix + '\x1b[2', 'A\x1b[2Kprompt$ '],
    writesAfterChunk: [[], [multilinePrefix]],
    finalWrites: [multilinePrefix, redrawTail],
  },
  {
    name: 'second destructive CSI split after bare ESC byte',
    chunks: [multilinePrefix + '\x1b[2A\x1b', '[2Kprompt$ '],
    writesAfterChunk: [[multilinePrefix], [multilinePrefix]],
    finalWrites: [multilinePrefix, redrawTail],
  },
  {
    name: 'second destructive CSI split after ESC[ prefix',
    chunks: [multilinePrefix + '\x1b[2A\x1b[', '2Kprompt$ '],
    writesAfterChunk: [[multilinePrefix], [multilinePrefix]],
    finalWrites: [multilinePrefix, redrawTail],
  },
  {
    name: 'second destructive CSI split mid-parameter sequence',
    chunks: [multilinePrefix + '\x1b[2A\x1b[2', 'Kprompt$ '],
    writesAfterChunk: [[multilinePrefix], [multilinePrefix]],
    finalWrites: [multilinePrefix, redrawTail],
  },
  {
    name: 'multiline output pending from previous chunk before redraw starts',
    chunks: [multilinePrefix, redrawTail],
    writesAfterChunk: [[], [multilinePrefix]],
    finalWrites: [multilinePrefix, redrawTail],
  },
];