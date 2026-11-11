// Entry point: bootstrap the Angular shell with the adapters chosen in providers.ts.

import "@fontsource-variable/sono/wght.css";
import { bootstrapApplication } from "@angular/platform-browser";
import { TerminalComponent } from "./adapters/angular/terminal.component";
import { appConfig } from "./providers";

bootstrapApplication(TerminalComponent, appConfig).catch((e) => {
  console.error(e);
  document.body.textContent = `tersh failed to start: ${e instanceof Error ? e.message : String(e)}`;
  document.body.classList.add("fatal");
});
