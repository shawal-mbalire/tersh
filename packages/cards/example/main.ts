// Run with `npx vite packages/cards/example` from the repo root. A real app
// would `import { mountCard } from "@tersh/cards"` and `import "@tersh/cards/styles.css"`.
import { mountCard } from "../src/index";
import notebook from "../../../examples/demos/showcase.ipynb?raw";

const $ = (id: string) => document.getElementById(id)!;

await mountCard($("markdown"), {
  kind: "markdown",
  data: 'Cards render **markdown**, $\\LaTeX$ like $e^{i\\pi} + 1 = 0$, and `code`:\n\n```rust\nfn main() { println!("hi"); }\n```',
});

// Video: any URL or data: URI, e.g. ?video=https://example.com/clip.webm
const video = new URLSearchParams(location.search).get("video");
if (video) await mountCard($("video"), { kind: "video", data: video });
else $("video").textContent = "Add ?video=URL to play a clip.";

await mountCard($("notebook"), { kind: "notebook", data: notebook });
document.title = "rendered";
