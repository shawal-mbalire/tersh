import { describe, expect, it, vi } from "vitest";
import { blobDocumentHost, mountCard } from "../src/index";
import { imageSource, mediaUrl } from "../src/util";

describe("@tersh/cards", () => {
  it("mounts, updates and disposes a card", async () => {
    const el = document.createElement("div");
    const card = await mountCard(el, { kind: "text", data: "hello" });
    expect(el.textContent).toBe("hello");
    await card.update({ data: "again" });
    expect(el.textContent).toBe("again");
    card.dispose();
    expect(el.childElementCount).toBe(0);
  });

  it("uses custom renderers and keeps instances across updates", async () => {
    const el = document.createElement("div");
    const update = vi.fn();
    const card = await mountCard(
      el,
      { kind: "shout", data: "hi" },
      {
        renderers: { shout: ({ el, data }) => ((el.textContent = String(data).toUpperCase()), { update }) },
      },
    );
    expect(el.textContent).toBe("HI");
    await card.update({ data: "yo" });
    expect(update).toHaveBeenCalledWith("yo", expect.objectContaining({ kind: "shout" }));
  });

  it("rejects unknown kinds", async () => {
    await expect(mountCard(document.createElement("div"), { kind: "nope" })).rejects.toThrow(/nope/);
  });

  it("passes URLs through and only releases blobs it made", () => {
    expect(imageSource("https://x.dev/a.png")).toBe("https://x.dev/a.png");
    expect(imageSource("./clip.webm", "video/webm")).toBe("./clip.webm");
    expect(imageSource("/9j/4AAQ", "image/jpeg")).toBe("data:image/jpeg;base64,/9j/4AAQ");

    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:made");
    mediaUrl("blob:theirs", "video/webm").release();
    expect(revoke).not.toHaveBeenCalled();
    const made = mediaUrl("AAAA", "video/webm");
    expect(made.url).toBe("blob:made");
    made.release();
    expect(revoke).toHaveBeenCalledWith("blob:made");
    vi.restoreAllMocks();
  });

  it("serves HTML documents from blob URLs by default", async () => {
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:doc");
    const host = blobDocumentHost();
    await host.put("a", "<p>hi</p>");
    expect(host.url("a")).toBe("blob:doc");
    host.drop("a");
    expect(host.url("a")).toBe("about:blank");
    vi.restoreAllMocks();
  });
});
