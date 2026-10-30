// Built-in card renderers. They use the same API as plugin renderers.

import type { CardRenderer } from "./types";
import type { Embed, PlotlyFigure } from "./embeds";
import { imageSource, joinText, mediaUrl, parseMaybeJson } from "./util";

const loadMarkdown = () => import("./markdown");
const loadEmbeds = () => import("./embeds");
const loadNotebook = () => import("./notebook");

const fixedHeight = (msg: { anchor?: { rows?: number; height?: unknown } }) =>
  msg.anchor?.rows !== undefined || msg.anchor?.height !== undefined;

const disposeAll = (embeds: Embed[]) => () => embeds.forEach((e) => e.dispose());

export const text: CardRenderer = ({ el, data }) => {
  const pre = document.createElement("pre");
  pre.className = "card-text";
  pre.textContent = joinText(data);
  el.append(pre);
};

export const json: CardRenderer = async ({ el, data }) => {
  const md = await loadMarkdown();
  const pre = document.createElement("pre");
  pre.className = "card-text hljs";
  pre.innerHTML = md.highlight(JSON.stringify(parseMaybeJson(data), null, 2), "json");
  el.append(pre);
};

export const markdown: CardRenderer = async ({ el, data, relayout }) => {
  const md = await loadMarkdown();
  const render = (value: unknown) => {
    el.classList.add("markdown");
    el.innerHTML = md.renderMarkdown(joinText(value));
    el.querySelectorAll("img").forEach((img) => img.addEventListener("load", relayout));
    relayout();
  };
  render(data);
  return { update: render };
};

export const image: CardRenderer = ({ el, data, message, relayout }) => {
  const img = document.createElement("img");
  img.className = "card-image";
  img.alt = message.title ?? "";
  img.addEventListener("load", relayout);
  const set = (value: unknown) => {
    const mime = typeof message.mime === "string" ? message.mime : "image/png";
    img.src = imageSource(value, mime);
  };
  set(data);
  el.append(img);
  return { update: set };
};

/** A video clip: a `data:` URI, or base64 with a `mime` field. Plays muted and
 *  looping by default; `autoplay`, `loop`, `muted` and `controls` override. */
export const video: CardRenderer = ({ el, data, message, relayout }) => {
  const flag = (name: string, fallback: boolean) => (typeof message[name] === "boolean" ? (message[name] as boolean) : fallback);
  const player = document.createElement("video");
  player.className = "card-video";
  player.autoplay = flag("autoplay", true);
  player.loop = flag("loop", true);
  player.muted = flag("muted", true);
  player.controls = flag("controls", true);
  player.playsInline = true;
  player.addEventListener("loadedmetadata", relayout);
  let url = "";
  const set = (value: unknown) => {
    if (url) URL.revokeObjectURL(url);
    url = mediaUrl(value, typeof message.mime === "string" ? message.mime : "video/webm");
    player.src = url;
  };
  set(data);
  el.append(player);
  return {
    update: set,
    dispose() {
      player.removeAttribute("src");
      player.load();
      URL.revokeObjectURL(url);
    },
  };
};

export const svg: CardRenderer = async ({ el, data, relayout }) => {
  const md = await loadMarkdown();
  const render = (value: unknown) => {
    el.innerHTML = md.sanitizeSvg(joinText(value));
    el.classList.add("card-svg");
    relayout();
  };
  render(data);
  return { update: render };
};

export const html: CardRenderer = async ({ el, data, message, relayout }) => {
  const { mountHtml } = await loadEmbeds();
  let embed = await mountHtml(el, joinText(data), { fixedHeight: fixedHeight(message), onResize: relayout });
  return {
    async update(next, msg) {
      embed.dispose();
      embed = await mountHtml(el, joinText(next), { fixedHeight: fixedHeight(msg), onResize: relayout });
    },
    dispose: () => embed.dispose(),
  };
};

export const plotly: CardRenderer = async ({ el, data, message }) => {
  const { mountPlotly } = await loadEmbeds();
  const plot = await mountPlotly(el, parseMaybeJson(data) as PlotlyFigure, fixedHeight(message));
  return {
    update: (next) => void plot.update(parseMaybeJson(next) as PlotlyFigure),
    dispose: () => plot.dispose(),
  };
};

/** A whole `.ipynb` document. */
export const notebook: CardRenderer = async ({ el, data, relayout }) => {
  const nb = await loadNotebook();
  const embeds = await nb.renderNotebook(el, parseMaybeJson(data) as never, relayout);
  return { dispose: disposeAll(embeds) };
};

/** One Jupyter output, a list of outputs, or a bare MIME bundle — for inline cell results (e.g. from Neovim). */
export const output: CardRenderer = async ({ el, data, relayout }) => {
  const nb = await loadNotebook();
  const value = parseMaybeJson(data);
  const list = Array.isArray(value) ? value : [value];
  const embeds: Embed[] = [];
  for (const item of list) {
    if (item && typeof item === "object" && "output_type" in item) {
      embeds.push(...(await nb.renderOutput(el, item as never, relayout)));
    } else {
      const box = document.createElement("div");
      box.className = "nb-output";
      el.append(box);
      embeds.push(...(await nb.renderBundle(box, (item ?? {}) as Record<string, unknown>, relayout)));
    }
  }
  relayout();
  return { dispose: disposeAll(embeds) };
};

export const builtinRenderers: Record<string, CardRenderer> = {
  text,
  json,
  markdown,
  image,
  video,
  svg,
  html,
  plotly,
  notebook,
  output,
};
