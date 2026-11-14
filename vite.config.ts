import { defineConfig } from "vite";

// Cross-origin isolation lets the voice (onnxruntime WASM) use several CPU
// threads instead of one. Keep in step with app.security.headers in tauri.conf.json.
const CROSS_ORIGIN_ISOLATION = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "credentialless",
};

// Tauri expects a fixed dev port and does not want the screen cleared.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
    headers: CROSS_ORIGIN_ISOLATION,
  },
  preview: { headers: CROSS_ORIGIN_ISOLATION },
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
