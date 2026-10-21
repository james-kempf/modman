// SPDX-License-Identifier: GPL-3.0-or-later
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    // Keep every asset a separate file: the CSP has no data:/inline allowances for scripts or styles.
    assetsInlineLimit: 0,
  },
});
