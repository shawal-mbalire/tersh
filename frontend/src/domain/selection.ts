// Text selection on the grid: pure geometry, in cells.

export interface Cell {
  x: number;
  y: number;
}

/** Where the drag started and where it is now; either may come first. */
export interface Selection {
  anchor: Cell;
  head: Cell;
}

/** One row of a selection: columns `from` up to (not including) `to`. */
export interface RowSpan {
  y: number;
  from: number;
  to: number;
}

/** Characters that end a word for double-click selection. */
const WORD_BREAK = /[\s"'`()[\]{}<>|;,]/;

/** The selection's two ends, first to last. */
export function ordered(s: Selection): [Cell, Cell] {
  const before = s.anchor.y < s.head.y || (s.anchor.y === s.head.y && s.anchor.x <= s.head.x);
  return before ? [s.anchor, s.head] : [s.head, s.anchor];
}

export function isEmpty(s: Selection): boolean {
  return s.anchor.x === s.head.x && s.anchor.y === s.head.y;
}

/** The selected columns of each row, for drawing and for copying. */
export function rowSpans(s: Selection, cols: number): RowSpan[] {
  const [start, end] = ordered(s);
  const spans: RowSpan[] = [];
  for (let y = start.y; y <= end.y; y++) {
    spans.push({ y, from: y === start.y ? start.x : 0, to: y === end.y ? end.x + 1 : cols });
  }
  return spans;
}

/** The word under column `x` of a row's text (double click), or null on a separator. */
export function wordAt(row: string, x: number, y: number): Selection | null {
  const text = [...row];
  const isWord = (ch: string | undefined) => !!ch && !WORD_BREAK.test(ch);
  if (!isWord(text[x])) return null;
  let start = x;
  let end = x;
  while (start > 0 && isWord(text[start - 1])) start--;
  while (end < text.length - 1 && isWord(text[end + 1])) end++;
  return { anchor: { x: start, y }, head: { x: end, y } };
}

/** The whole row `y` (triple click). */
export function lineAt(y: number, cols: number): Selection {
  return { anchor: { x: 0, y }, head: { x: cols - 1, y } };
}
