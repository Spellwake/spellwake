import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { vanillaExtractPlugin } from "@vanilla-extract/vite-plugin";

export default defineConfig({
  plugins: [react(), vanillaExtractPlugin()],
  base: "/components-next/",
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 6202,
    strictPort: true,
  },
});
