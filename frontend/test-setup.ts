// Preloaded by `bun test` (bunfig.toml): give the tests a DOM and let modules
// that import style sheets load. Styles don't matter to the tests.

import { GlobalRegistrator } from "@happy-dom/global-registrator";
import { plugin } from "bun";

GlobalRegistrator.register({ url: "http://localhost/" });

plugin({
  name: "stub css",
  setup(build) {
    build.onLoad({ filter: /\.css$/ }, () => ({ contents: "", loader: "js" }));
  },
});
