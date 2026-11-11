// Markdown + LaTeX + code highlighting. Loaded lazily the first time a card needs it.

import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/common";
import "highlight.js/styles/github-dark.css";
import "katex/dist/katex.min.css";
import katex from "katex";
import { Marked } from "marked";
import markedKatex from "marked-katex-extension";

const marked = new Marked(markedKatex({ throwOnError: false, nonStandard: true }), {
  gfm: true,
  renderer: {
    code({ text, lang }) {
      return `<pre><code class="hljs">${highlight(text, lang)}</code></pre>`;
    },
  },
});

export function sanitize(html: string): string {
  return DOMPurify.sanitize(html, { ADD_ATTR: ["target"], FORBID_TAGS: ["style"] });
}

export function renderMarkdown(source: string): string {
  return sanitize(marked.parse(source, { async: false }));
}

/** Render a LaTeX block (notebook `text/latex` output). */
export function renderLatex(source: string): string {
  const body = source.trim().replace(/^\$\$?|\$\$?$/g, "");
  return katex.renderToString(body, { displayMode: true, throwOnError: false });
}

/** Syntax-highlight code, returning escaped HTML. */
export function highlight(code: string, lang?: string): string {
  const language = lang?.trim().split(/\s/)[0].toLowerCase();
  try {
    if (language && hljs.getLanguage(language)) return hljs.highlight(code, { language }).value;
    if (!language) return hljs.highlightAuto(code).value;
  } catch {
    // Fall through to plain text.
  }
  return escapeHtml(code);
}

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

export function sanitizeSvg(svg: string): string {
  return DOMPurify.sanitize(svg, { USE_PROFILES: { svg: true, svgFilters: true } });
}
