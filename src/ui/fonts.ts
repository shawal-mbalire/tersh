// Getting fonts ready before the grid is measured and painted.

import { SYMBOLS_SAMPLE, splitFamilies, unquote, withSymbols } from "../domain/fonts";
import type { Config } from "../domain/types";

/** Load the configured text font and the bundled icon font. */
export async function loadFonts({ family, size }: Config["font"]) {
  const list = withSymbols(family);
  const font = `${size}px ${list}`;
  await Promise.all([document.fonts.load(font, "M"), document.fonts.load(font, SYMBOLS_SAMPLE)]).catch(() => undefined);
  // WebKit resolves load() before unicode-range faces (the icon font) finish, so load those explicitly.
  const wanted = new Set(splitFamilies(list).map(unquote));
  const faces = [...document.fonts].filter((face) => wanted.has(unquote(face.family)));
  await Promise.all(faces.map((face) => face.load().catch(() => undefined)));
}
