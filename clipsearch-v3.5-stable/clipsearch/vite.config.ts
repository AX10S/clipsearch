import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

// Vite config tuned for Tauri v2.
// - `clearScreen:false` so Rust compiler output isn't swallowed.
// - `server.host` is `true` so mobile dev can reach it (harmless on desktop).
// - `server.strictPort` so Tauri doesn't grab the wrong port.
// - `envPrefix` whitelists TAURI_* env vars for the frontend.
export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // Vite options tailored for Tauri development; see https://vitejs.dev/config/
  clearScreen: false,
  server: {
    host: true,
    port: 1420,
    strictPort: true,
    hmr: {
      protocol: "ws",
      host: "localhost",
      port: 1421,
    },
    watch: {
      // Don't watch the Rust source — that's `cargo`'s job.
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
  },
}));
