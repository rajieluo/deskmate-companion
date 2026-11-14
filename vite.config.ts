import { defineConfig } from "vite";

// Tauri expects a fixed dev port and does not want the screen cleared.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  // The text-to-speech worker uses ES module imports.
  worker: { format: "es" },
  build: {
    target: "es2021",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // Two pages: the avatar window and the Chat history window.
    rollupOptions: { input: { main: "index.html", history: "history.html" } },
  },
});
