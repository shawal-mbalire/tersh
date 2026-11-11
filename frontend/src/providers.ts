// Composition root: which adapter sits behind each port. Swap one here (a
// fake backend for a browser demo, a different screen) and nothing else changes.

import { provideZonelessChangeDetection, type ApplicationConfig } from "@angular/core";
import { provideTersh } from "./adapters/angular/tokens";
import { TauriBackend } from "./adapters/tauri/backend";

export const appConfig: ApplicationConfig = {
  providers: [provideZonelessChangeDetection(), provideTersh({ backend: () => new TauriBackend() })],
};
