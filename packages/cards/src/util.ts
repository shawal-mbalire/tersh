/** Escape text for inclusion in HTML. Kept here (not in markdown.ts) so it doesn't pull in the lazy chunk. */
export function escapeText(text: string): string {
  return text.replace(/[&<>]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;"));
}

/** Join notebook-style multiline strings (`string | string[]`). */
export function joinText(value: unknown): string {
  if (Array.isArray(value)) return value.join("");
  if (typeof value === "string") return value;
  if (value === undefined || value === null) return "";
  return JSON.stringify(value, null, 2);
}

/** Accepts a URL (`data:`, `blob:`, `http(s):` or `./relative`), a base64 string plus mime type, or `{data|src, mime}`. */
export function imageSource(data: unknown, mime = "image/png"): string {
  if (data && typeof data === "object" && !Array.isArray(data)) {
    const obj = data as Record<string, unknown>;
    return imageSource(obj.src ?? obj.data, typeof obj.mime === "string" ? obj.mime : mime);
  }
  const text = joinText(data).trim();
  if (/^(data:|blob:|https?:\/\/|\.\.?\/)/.test(text)) return text;
  if (mime === "image/svg+xml" && text.startsWith("<")) return `data:${mime};charset=utf-8,${encodeURIComponent(text)}`;
  return `data:${mime};base64,${text.replace(/\s+/g, "")}`;
}

/**
 * A URL for media sent like an image (see `imageSource`). Inline data plays
 * from a `blob:` URL rather than a multi-megabyte `data:` URI; `release` frees
 * it (other URLs are left alone).
 */
export function mediaUrl(data: unknown, mime: string): { url: string; release(): void } {
  const source = imageSource(data, mime);
  const match = /^data:([^;,]*)(;base64)?,/.exec(source);
  if (!match) return { url: source, release() {} };
  const body = source.slice(match[0].length);
  const bytes = match[2] ? Uint8Array.from(atob(body), (c) => c.charCodeAt(0)) : new TextEncoder().encode(decodeURIComponent(body));
  const url = URL.createObjectURL(new Blob([bytes], { type: match[1] || mime }));
  return { url, release: () => URL.revokeObjectURL(url) };
}

export function parseMaybeJson(data: unknown): unknown {
  if (typeof data !== "string") return data;
  try {
    return JSON.parse(data);
  } catch {
    return data;
  }
}
