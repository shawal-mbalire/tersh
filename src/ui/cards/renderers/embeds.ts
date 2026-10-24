// Embeds shared by several renderers: isolated HTML documents and Plotly figures.

import { cardDocuments } from "../documents";

export interface Embed {
  el: HTMLElement;
  dispose(): void;
}

let docCounter = 0;

function cssVar(name: string, fallback: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

/** Reports the document height to the parent so auto-height cards can fit their content. */
const HEIGHT_REPORTER = (token: string) => `<script>(function(){
  var token = ${JSON.stringify(token)};
  function post(){ parent.postMessage({ tershFrame: token, height: document.documentElement.scrollHeight }, "*"); }
  if (window.ResizeObserver) new ResizeObserver(post).observe(document.documentElement);
  addEventListener("load", post); post();
})();</script>`;

/**
 * Show untrusted HTML in a sandboxed iframe served from the `tersh-card:`
 * origin (see src-tauri/src/cards.rs). Scripts run, but can't touch the
 * terminal, Tauri IPC, or (by default) the network.
 *
 * With `fixedHeight` the frame fills its container; otherwise it grows to fit.
 */
export async function mountHtml(
  container: HTMLElement,
  html: string,
  opts: { fixedHeight: boolean; onResize?: () => void },
): Promise<Embed> {
  const token = `doc-${Date.now().toString(36)}-${++docCounter}`;
  const base = `<!doctype html><meta charset="utf-8"><style>
    :root { color-scheme: dark; }
    html, body { margin: 0; background: transparent; color: ${cssVar("--fg", "#ddd")}; font: 13px system-ui, sans-serif; }
    body { padding: 6px; overflow: ${opts.fixedHeight ? "auto" : "hidden"}; }
    a { color: ${cssVar("--accent", "#3b8eea")}; }
    table { border-collapse: collapse; } td, th { border: 1px solid #8884; padding: 2px 6px; }
  </style>${HEIGHT_REPORTER(token)}`;
  const documents = cardDocuments();
  await documents.put(token, base + html);

  const iframe = document.createElement("iframe");
  iframe.className = "embed-html";
  iframe.setAttribute("sandbox", "allow-scripts");
  iframe.referrerPolicy = "no-referrer";
  iframe.src = documents.url(token);
  if (opts.fixedHeight) iframe.style.height = "100%";
  else iframe.style.height = "24px";

  const onMessage = (event: MessageEvent) => {
    if (event.source !== iframe.contentWindow) return;
    const msg = event.data as { tershFrame?: string; height?: number };
    if (msg?.tershFrame !== token || typeof msg.height !== "number" || opts.fixedHeight) return;
    iframe.style.height = `${Math.min(Math.ceil(msg.height), 4000)}px`;
    opts.onResize?.();
  };
  window.addEventListener("message", onMessage);
  container.append(iframe);

  return {
    el: iframe,
    dispose() {
      window.removeEventListener("message", onMessage);
      iframe.remove();
      documents.drop(token);
    },
  };
}

export interface PlotlyFigure {
  data?: unknown[];
  layout?: Record<string, unknown>;
  config?: Record<string, unknown>;
  frames?: unknown[];
}

type PlotlyModule = {
  newPlot(el: HTMLElement, data: unknown[], layout?: object, config?: object): Promise<unknown>;
  react(el: HTMLElement, data: unknown[], layout?: object, config?: object): Promise<unknown>;
  purge(el: HTMLElement): void;
  Plots: { resize(el: HTMLElement): void };
};

let plotly: Promise<PlotlyModule> | null = null;
const loadPlotly = () => (plotly ??= import("plotly.js-dist-min").then((m) => (m.default ?? m) as unknown as PlotlyModule));

function plotArgs(figure: PlotlyFigure): [unknown[], object, object] {
  const fg = cssVar("--fg", "#ddd");
  const layout = {
    paper_bgcolor: "rgba(0,0,0,0)",
    plot_bgcolor: "rgba(0,0,0,0)",
    font: { color: fg },
    margin: { t: 32, r: 16, b: 40, l: 48 },
    ...figure.layout,
    autosize: true,
  };
  const config = { responsive: true, displaylogo: false, ...figure.config };
  return [figure.data ?? [], layout, config];
}

export async function mountPlotly(container: HTMLElement, figure: PlotlyFigure, fixedHeight: boolean) {
  const Plotly = await loadPlotly();
  const el = document.createElement("div");
  el.className = "embed-plotly";
  el.style.height = fixedHeight ? "100%" : `${Number(figure.layout?.height) || 380}px`;
  container.append(el);
  await Plotly.newPlot(el, ...plotArgs(figure));
  const observer = new ResizeObserver(() => Plotly.Plots.resize(el));
  observer.observe(el);
  return {
    el,
    update: (next: PlotlyFigure) => Plotly.react(el, ...plotArgs(next)),
    dispose() {
      observer.disconnect();
      Plotly.purge(el);
      el.remove();
    },
  };
}
