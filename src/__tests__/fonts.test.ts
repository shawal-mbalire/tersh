import { describe, expect, it } from "vitest";
import { splitFamilies, withSymbols } from "../domain/fonts";

describe("font lists", () => {
  it("splits on commas outside quotes", () => {
    expect(splitFamilies(`'Sono Variable', "A, B", monospace`)).toEqual([`'Sono Variable'`, `"A, B"`, "monospace"]);
  });

  it("adds the icon font before the generic family", () => {
    expect(withSymbols("'Sono Variable', monospace")).toBe("'Sono Variable', 'tersh Symbols', monospace");
    expect(withSymbols("'JetBrainsMono Nerd Font'")).toBe("'JetBrainsMono Nerd Font', 'tersh Symbols', monospace");
    expect(withSymbols("Hack, 'tersh Symbols', monospace")).toBe("Hack, 'tersh Symbols', monospace");
  });
});
