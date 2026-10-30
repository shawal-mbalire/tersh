# @tersh/cards

The renderers the [tersh](https://github.com/shawal-mbalire/tersh) terminal
draws rich cards with, for any web page: Jupyter notebooks and outputs,
markdown with KaTeX, interactive Plotly, sandboxed HTML, SVG, images, video,
JSON and text.

```ts
import { mountCard } from "@tersh/cards";
import "@tersh/cards/styles.css";

const el = document.getElementById("preview")!;
const card = await mountCard(el, { kind: "notebook", data: await (await fetch("/analysis.ipynb")).json() });

await card.update({ data: updatedNotebook }); // same kind: updated in place where the renderer supports it
await card.update({ kind: "markdown", data: "# Done" }); // another kind: re-rendered
card.dispose();
```

## Kinds

| Kind       | `data`                                                                                    |
| ---------- | ----------------------------------------------------------------------------------------- |
| `notebook` | an nbformat 4 notebook (object or JSON string)                                            |
| `output`   | one Jupyter output, a list of them, or a bare MIME bundle                                 |
| `markdown` | string; `$...$` and `$$...$$` are rendered with KaTeX                                     |
| `plotly`   | `{ data, layout, config? }`                                                               |
| `html`     | a document, run in a sandboxed iframe                                                     |
| `svg`      | SVG markup (sanitised)                                                                    |
| `image`    | a URL, a `data:` URI, or base64 with `mime`                                               |
| `video`    | like `image`. Muted and looping unless `autoplay`/`loop`/`muted`/`controls` say otherwise |
| `json`     | any value, pretty-printed and highlighted                                                 |
| `text`     | string                                                                                    |

## API

- `mountCard(container, message, { renderers?, onResize? })`: renders
  `message` into `container`. Resolves to `{ update(message), dispose() }`.
  Pass `renderers` to add kinds or replace built-in ones. `onResize` fires
  when content changes height, for example when an image loads.
- `builtinRenderers`: the renderers by kind. Each is a `CardRenderer`, the
  same type tersh's [JavaScript plugins](../../docs/plugins.md) register.
- `setDocumentHost(host)`: where HTML documents are served from. By default
  they are `blob:` URLs in iframes with `sandbox="allow-scripts"`.
- `ansiToHtml`, `pickMime`, `MIME_PRIORITY`: helpers for Jupyter outputs.

Heavy dependencies (KaTeX, highlight.js, Plotly) load lazily, the first time a
card needs them.

## Theming

Card content uses CSS variables. They default to Catppuccin Mocha; set them on
`:root` or on a container:

```css
:root {
  --fg: #4c4f69;
  --accent: #8839ef;
  --code-bg: #e6e9ef;
  --font-family: "JetBrains Mono", monospace;
  --ansi-1: #d20f39; /* --ansi-0 … --ansi-15 colour terminal output */
}
```

## Development

```sh
npx vite packages/cards/example        # example page rendering the demo notebook
cd packages/cards && npm run build     # dist/: ES modules, styles.css, .d.ts
```

Inside the tersh repository the app builds this package from source, so a
change here shows up in tersh and in the example.
