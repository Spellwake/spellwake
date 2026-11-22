import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import { designPixels } from "./scripts/design-pixels";
import react from "@vitejs/plugin-react";
import { vanillaExtractPlugin } from "@vanilla-extract/vite-plugin";

const mainEntry = fileURLToPath(new URL("./index.html", import.meta.url));

export default defineConfig(({ mode }) => ({
  plugins: [react(), vanillaExtractPlugin()],
  base: mode === "tauri" || mode === "web" ? "/" : "/play/",
  clearScreen: false,
  css: { postcss: { plugins: [designPixels()] } },
  build: {
    outDir: mode === "tauri" ? "dist-tauri" : "dist",
    rollupOptions: {
      input: {
        app: mainEntry,
        privacy: fileURLToPath(
          new URL("./privacy/index.html", import.meta.url),
        ),
      },
    },
  },
  server: {
    host: process.env.TAURI_DEV_HOST || "127.0.0.1",
    port: 6204,
    strictPort: true,
    ...(process.env.TAURI_DEV_HOST
      ? {
          hmr: { protocol: "ws", host: process.env.TAURI_DEV_HOST, port: 6205 },
        }
      : {}),
    watch: { ignored: ["**/src-tauri/**", "**/dist/**", "**/dist-tauri/**"] },
  },
}));
