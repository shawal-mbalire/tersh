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

/** Accepts a data URL, a base64 string plus mime type, or `{data|src, mime}`. */
export function imageSource(data: unknown, mime = "image/png"): string {
  if (data && typeof data === "object" && !Array.isArray(data)) {
    const obj = data as Record<string, unknown>;
    return imageSource(obj.src ?? obj.data, typeof obj.mime === "string" ? obj.mime : mime);
  }
  const text = joinText(data).trim();
  if (text.startsWith("data:") || text.startsWith("blob:")) return text;
  if (mime === "image/svg+xml" && text.startsWith("<")) return `data:${mime};charset=utf-8,${encodeURIComponent(text)}`;
  return `data:${mime};base64,${text.replace(/\s+/g, "")}`;
}

export function parseMaybeJson(data: unknown): unknown {
  if (typeof data !== "string") return data;
  try {
    return JSON.parse(data);
  } catch {
    return data;
  }
}
