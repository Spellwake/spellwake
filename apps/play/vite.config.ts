import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

const shell = fileURLToPath(
  new URL(
    "../../benchmarks/browser-rendering/public/play-area.html",
    import.meta.url,
  ),
);
const mainEntry = fileURLToPath(new URL("./index.html", import.meta.url));

/** Keep the native and study views on one renderer-owned DOM and stylesheet. */
function tabletopShell(): Plugin {
  return {
    name: "spellwake-tabletop-shell",
    buildStart() {
      this.addWatchFile(shell);
    },
    configureServer(server) {
      server.watcher.add(shell);
      const reload = (path: string): void => {
        if (path === shell) server.ws.send({ type: "full-reload" });
      };
      server.watcher.on("change", reload);
      server.httpServer?.once("close", () =>
        server.watcher.off("change", reload),
      );
    },
    transformIndexHtml: {
      order: "pre",
      async handler(_html, context) {
        if (context.filename !== mainEntry) return;
        const html = await readFile(shell, "utf8");
        const entry =
          '<script type="module" src="/build/play-area-entry.js"></script>';
        if (!html.includes(entry)) {
          throw new Error(
            "The renderer shell entry changed; update the play app binding.",
          );
        }
        return html
          .replace(entry, '<script type="module" src="/src/main.ts"></script>')
          .replace(
            "<title>Spellwake · Play</title>",
            "<title>Spellwake · Sample tabletop</title>",
          );
      },
    },
  };
}

export default defineConfig(({ mode }) => ({
  plugins: [tabletopShell()],
  base: mode === "tauri" || mode === "web" ? "/" : "/play/",
  clearScreen: false,
  build: {
    outDir: mode === "tauri" ? "dist-tauri" : "dist",
    rollupOptions: {
      input: {
        table: mainEntry,
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
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
