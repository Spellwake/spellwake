import { importCatalogue } from "./scripts/import-catalogue.ts";
import { reviewCatalogue } from "./scripts/review-catalogue.ts";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { vanillaExtractPlugin } from "@vanilla-extract/vite-plugin";
import { fileURLToPath } from "node:url";

export default defineConfig({
  base: "./",
  plugins: [
    react(),
    vanillaExtractPlugin(),
    reviewCatalogue(),
    importCatalogue(),
  ],
  build: {
    outDir: fileURLToPath(
      new URL(
        "../../assets/ui-review/open-play/approved-screens/controls/",
        import.meta.url,
      ),
    ),
    emptyOutDir: true,
    cssCodeSplit: false,
    assetsInlineLimit: 1000000,
    rollupOptions: {
      output: {
        entryFileNames: "gallery.js",
        assetFileNames: "gallery.[ext]",
      },
    },
  },
});
