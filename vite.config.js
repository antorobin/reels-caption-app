import { resolve } from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and relative asset paths
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "esnext",
    outDir: "dist",
    rollupOptions: {
      // Two real windows share this one Vite app: the main UI, and the
      // floating mic-dictation HUD (dictation.rs opens it at
      // `WebviewUrl::App("dictation-hud.html")`) -- splashscreen.html
      // doesn't need an entry here since it's a plain static file with no
      // build step (see public/splashscreen.html).
      input: {
        main: resolve(__dirname, "index.html"),
        "dictation-hud": resolve(__dirname, "dictation-hud.html"),
      },
    },
  },
});
