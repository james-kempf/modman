// SPDX-License-Identifier: GPL-3.0-or-later
// Entry: pick the transport (Tauri, or the demo backend under plain
// `vite dev`), register event listeners + app_ready once, then render.

import { QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { startBridge } from "./app/bridge";
import { makeQueryClient } from "./app/queries";
import { RootBoundary } from "./components/shell/ErrorBoundary";
// Bundled fonts (no remote origins under the CSP).
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./index.css";
import { setTransport } from "./ipc/transport";

async function main() {
  if (!("__TAURI_INTERNALS__" in window)) {
    const { createDemoBackend } = await import("./mock/demo");
    setTransport(createDemoBackend());
  }
  const qc = makeQueryClient();
  // Outside React so StrictMode's double effects cannot call app_ready twice.
  void startBridge(qc);
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <QueryClientProvider client={qc}>
        <RootBoundary>
          <App />
        </RootBoundary>
      </QueryClientProvider>
    </StrictMode>,
  );
}

void main();
