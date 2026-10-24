// Jupyter notebooks (nbformat 4) and individual cell outputs.

import { ansiToHtml } from "./ansi";
import { type Embed, mountHtml, mountPlotly, type PlotlyFigure } from "./embeds";
import { imageSource, joinText, parseMaybeJson } from "./util";

export interface NotebookOutput {
  output_type: "stream" | "execute_result" | "display_data" | "error" | "update_display_data";
  name?: string;
  text?: string | string[];
  data?: Record<string, unknown>;
  ename?: string;
  evalue?: string;
  traceback?: string[];
  execution_count?: number | null;
}

export interface NotebookCell {
  cell_type: "code" | "markdown" | "raw";
  source: string | string[];
  outputs?: NotebookOutput[];
  execution_count?: number | null;
}

export interface Notebook {
  cells: NotebookCell[];
  metadata?: {
    language_info?: { name?: string };
    kernelspec?: { language?: string; display_name?: string };
  };
}

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

type Markdown = typeof import("./markdown");
let markdown: Promise<Markdown> | null = null;
const loadMarkdown = () => (markdown ??= import("./markdown"));

/** Renders outputs into `container`; returns embeds to dispose. */
export async function renderOutput(container: HTMLElement, output: NotebookOutput, relayout: () => void): Promise<Embed[]> {
  const el = document.createElement("div");
  el.className = `nb-output nb-${output.output_type}`;
  container.append(el);

  switch (output.output_type) {
    case "stream": {
      const pre = document.createElement("pre");
      if (output.name === "stderr") pre.classList.add("nb-stderr");
      pre.innerHTML = ansiToHtml(joinText(output.text));
      el.append(pre);
      return [];
    }
    case "error": {
      const pre = document.createElement("pre");
      pre.className = "nb-traceback";
      const traceback = output.traceback?.length ? output.traceback.join("\n") : `${output.ename}: ${output.evalue}`;
      pre.innerHTML = ansiToHtml(traceback);
      el.append(pre);
      return [];
    }
    default:
      return renderBundle(el, output.data ?? {}, relayout);
  }
}

export async function renderBundle(el: HTMLElement, bundle: Record<string, unknown>, relayout: () => void): Promise<Embed[]> {
  const mime = pickMime(bundle);
  if (!mime) {
    el.textContent = `[unsupported output: ${Object.keys(bundle).join(", ") || "empty"}]`;
    el.classList.add("nb-unsupported");
    return [];
  }
  const value = bundle[mime];
  switch (mime) {
    case "application/vnd.plotly.v1+json": {
      const plot = await mountPlotly(el, parseMaybeJson(value) as PlotlyFigure, false);
      return [plot];
    }
    case "text/html":
      return [await mountHtml(el, joinText(value), { fixedHeight: false, onResize: relayout })];
    case "image/svg+xml":
    case "image/png":
    case "image/jpeg":
    case "image/gif": {
      const img = document.createElement("img");
      img.src = imageSource(value, mime);
      img.addEventListener("load", relayout);
      el.append(img);
      return [];
    }
    case "text/markdown": {
      const md = await loadMarkdown();
      el.classList.add("markdown");
      el.innerHTML = md.renderMarkdown(joinText(value));
      return [];
    }
    case "text/latex": {
      const md = await loadMarkdown();
      el.innerHTML = md.renderLatex(joinText(value));
      return [];
    }
    default: {
      const pre = document.createElement("pre");
      pre.innerHTML = ansiToHtml(mime === "application/json" ? JSON.stringify(value, null, 2) : joinText(value));
      el.append(pre);
      return [];
    }
  }
}

export async function renderNotebook(container: HTMLElement, nb: Notebook, relayout: () => void): Promise<Embed[]> {
  if (!nb || !Array.isArray(nb.cells)) throw new Error("not a notebook: expected an object with a `cells` array");
  const md = await loadMarkdown();
  const language = nb.metadata?.language_info?.name ?? nb.metadata?.kernelspec?.language ?? "python";
  const embeds: Embed[] = [];

  const root = document.createElement("div");
  root.className = "nb";
  container.append(root);

  for (const [index, cell] of nb.cells.entries()) {
    const el = document.createElement("section");
    el.className = `nb-cell nb-${cell.cell_type}`;
    el.dataset.index = String(index);
    root.append(el);
    const source = joinText(cell.source);

    if (cell.cell_type === "markdown") {
      el.classList.add("markdown");
      el.innerHTML = md.renderMarkdown(source);
      continue;
    }
    if (cell.cell_type === "raw") {
      const pre = document.createElement("pre");
      pre.textContent = source;
      el.append(pre);
      continue;
    }

    const input = document.createElement("div");
    input.className = "nb-input";
    const prompt = document.createElement("div");
    prompt.className = "nb-prompt";
    prompt.textContent = `[${cell.execution_count ?? " "}]`;
    const code = document.createElement("pre");
    code.className = "nb-source hljs";
    code.innerHTML = md.highlight(source, language);
    input.append(prompt, code);
    el.append(input);

    if (cell.outputs?.length) {
      const outputs = document.createElement("div");
      outputs.className = "nb-outputs";
      el.append(outputs);
      for (const output of cell.outputs) embeds.push(...(await renderOutput(outputs, output, relayout)));
    }
  }
  return embeds;
}
