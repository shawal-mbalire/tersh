// Which MIME type of a Jupyter output bundle to render.

/** Rich MIME types we can render, best first (mirrors VS Code / JupyterLab priority). */
export const MIME_PRIORITY = [
  "application/vnd.plotly.v1+json",
  "text/html",
  "image/svg+xml",
  "image/png",
  "image/jpeg",
  "image/gif",
  "text/markdown",
  "text/latex",
  "application/json",
  "text/plain",
] as const;

export function pickMime(bundle: Record<string, unknown>): string | null {
  return MIME_PRIORITY.find((m) => m in bundle) ?? null;
}
