/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] } },
  build: { rollupOptions: { input: { pill: "pill.html", panel: "panel.html" } } },
  test: { environment: "jsdom", include: ["src/**/*.test.{ts,tsx}"], passWithNoTests: true, setupFiles: ["src/test-setup.ts"] },
});
