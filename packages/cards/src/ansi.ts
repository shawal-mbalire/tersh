// Minimal SGR → HTML conversion for notebook tracebacks and stream output.

import { escapeText } from "./util";

const NAMES = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

export function ansiToHtml(text: string): string {
  let out = "";
  let fg: string | null = null;
  let bold = false;
  const re = /\x1b\[([\d;]*)m/g;
  let last = 0;

  const flush = (chunk: string) => {
    if (!chunk) return;
    const classes = [fg ? `ansi-${fg}` : "", bold ? "ansi-bold" : ""].filter(Boolean).join(" ");
    out += classes ? `<span class="${classes}">${escapeText(chunk)}</span>` : escapeText(chunk);
  };

  for (let m = re.exec(text); m; m = re.exec(text)) {
    flush(text.slice(last, m.index));
    last = re.lastIndex;
    const codes = m[1] === "" ? [0] : m[1].split(";").map(Number);
    for (let i = 0; i < codes.length; i++) {
      const c = codes[i];
      if (c === 0) [fg, bold] = [null, false];
      else if (c === 1) bold = true;
      else if (c === 22) bold = false;
      else if (c >= 30 && c <= 37) fg = NAMES[c - 30];
      else if (c >= 90 && c <= 97) fg = `bright-${NAMES[c - 90]}`;
      else if (c === 39) fg = null;
      else if (c === 38 || c === 48) i += codes[i + 1] === 5 ? 2 : 4; // skip extended colours
    }
  }
  flush(text.slice(last));
  return out;
}
