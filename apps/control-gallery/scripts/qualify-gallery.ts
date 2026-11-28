/**
 * Standalone Playwright smoke test for the Spellwake Component Gallery
 * (`apps/control-gallery`, Rendering -> `component-gallery` subdomain).
 *
 * Verifies that representative @spellwake/ui specimens, lazy-loaded match
 * specimens, and the Combat Damage Walkthrough all mount and render with
 * zero uncaught page errors, even in a public checkout without private
 * `spellwake-assets` or `assets/ui-review` artwork files.
 *
 * Usage: node apps/control-gallery/scripts/qualify-gallery.ts
 */
import { existsSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { vanillaExtractPlugin } from "@vanilla-extract/vite-plugin";
import { createServer, type Plugin, type ViteDevServer } from "vite";
import {
  assertCondition,
  launchBrowser,
  type CheckResult,
} from "../../play/scripts/playwright-harness.ts";

const PLACEHOLDER_SVG_RAW =
  "<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64'><rect width='64' height='64' fill='#282c2a'/></svg>";
const PLACEHOLDER_SVG_DATA_URL = `data:image/svg+xml;utf8,${encodeURIComponent(PLACEHOLDER_SVG_RAW)}`;

const FALLBACK_IMPORT_CATALOGUE = [
  "Lightning Bolt\t4457ed35-7c10-48c8-9776-456485fdf070\ta207919d-6699-41f3-b974-414b467b8756\tmsc\t806\t\t\tInstant\t1",
  "Goblin Guide\tc0e5e4d2-8a4a-4e82-9a22-7c9f01407c01\t2a765913-0295-4518-8884-725216f3895d\tzen\t126\t\t\tCreature — Goblin Scout\t1",
].join("\n");

/**
 * Provides safe local fallbacks for private review/image assets and virtual
 * catalogues that live outside the public code repo so `@spellwake/ui`
 * components still mount and execute in full.
 */
function gallerySmokeFallbacksPlugin(galleryRootDir: string): Plugin {
  const virtualAssetPrefix = "\0missing-private-asset:";
  const approvedId = "virtual:approved-screens";
  const importId = "virtual:deck-import-catalogue";

  return {
    name: "spellwake-gallery-smoke-fallbacks",
    enforce: "pre",
    resolveId(source, importer) {
      if (source === approvedId) return `\0${approvedId}`;
      if (source === importId) return `\0${importId}`;
      if (!source.startsWith(".") && !source.startsWith("/")) return null;
      const [cleanPath = source, query = ""] = source.split("?");
      if (!/\.(svg|png|webp|jpe?g)$/i.test(cleanPath)) return null;
      const resolved =
        source.startsWith(".") && importer
          ? resolve(dirname(importer), cleanPath)
          : resolve(galleryRootDir, cleanPath.replace(/^\/+/, ""));
      if (existsSync(resolved)) return null;
      const isRaw = query.split("&").includes("raw");
      return `${virtualAssetPrefix}${isRaw ? "raw:" : "url:"}${cleanPath}`;
    },
    load(id) {
      if (id === `\0${approvedId}`) {
        return "export default [];";
      }
      if (id === `\0${importId}`) {
        return `export default ${JSON.stringify(FALLBACK_IMPORT_CATALOGUE)};`;
      }
      if (!id.startsWith(virtualAssetPrefix)) return null;
      const isRaw = id.startsWith(`${virtualAssetPrefix}raw:`);
      return `export default ${JSON.stringify(isRaw ? PLACEHOLDER_SVG_RAW : PLACEHOLDER_SVG_DATA_URL)};`;
    },
  };
}

const VIEWS = [
  {
    name: "gallery default (DL-01 Deck library)",
    query: "",
    readySelector: "main",
  },
  {
    name: "gallery specimen (EX-01 Predictions)",
    query: "?view=EX-01",
    readySelector: "main",
  },
  {
    name: "gallery specimen (IMP-02 Import deck)",
    query: "?view=IMP-02",
    readySelector: "main",
  },
  {
    name: "gallery specimen (CM-06 Table settings)",
    query: "?view=CM-06",
    readySelector: "main",
  },
  {
    name: "gallery specimen (AS-R03 Picture zoom)",
    query: "?view=AS-R03",
    readySelector: "main",
  },
  {
    name: "gallery lazy specimen (AS01:stack Spell stack)",
    query: "?view=AS01:stack",
    readySelector: "main",
  },
  {
    name: "combat damage walkthrough",
    query: "?walkthrough=combat-damage",
    readySelector: "#root > *",
  },
] as const;

const galleryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tempDir = await mkdtemp(join(tmpdir(), "spellwake-qualify-gallery-"));

const server: ViteDevServer = await createServer({
  configFile: false,
  root: galleryRoot,
  cacheDir: join(tempDir, "vite-cache"),
  plugins: [
    gallerySmokeFallbacksPlugin(galleryRoot),
    react(),
    vanillaExtractPlugin(),
  ],
  server: { port: 0, strictPort: false, host: "127.0.0.1" },
  logLevel: "error",
});

const checks: CheckResult[] = [];
const runCheck = async (name: string, fn: () => Promise<void>): Promise<void> => {
  try {
    await fn();
    checks.push({ name, pass: true });
    console.log(`(pass) ${name}`);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    checks.push({ name, pass: false, detail });
    console.error(`(fail) ${name}: ${detail}`);
  }
};

let browser: Awaited<ReturnType<typeof launchBrowser>> | undefined;
try {
  await server.listen();
  const address = server.httpServer?.address();
  if (!address || typeof address === "string") {
    throw new Error("Could not resolve control-gallery Vite server port.");
  }
  const origin = `http://127.0.0.1:${address.port}/`;
  browser = await launchBrowser();

  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    reducedMotion: "reduce",
  });
  try {
    const page = await context.newPage();
    page.setDefaultTimeout(20_000);
    page.setDefaultNavigationTimeout(45_000);
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => {
      pageErrors.push(err.message);
      console.error(`[gallery pageerror] ${err.message}`);
    });
    page.on("response", (res) => {
      if (res.status() >= 400 && !res.url().includes("favicon")) {
        console.error(`[gallery http ${res.status()}] ${res.url()}`);
      }
    });

    for (const view of VIEWS) {
      await runCheck(
        `${view.name}: mounts and renders with zero page errors`,
        async () => {
          const errorsBefore = pageErrors.length;
          await page.goto(`${origin}${view.query}`);
          await page.locator(view.readySelector).first().waitFor();
          await page.waitForTimeout(250);
          const metrics = await page.evaluate(() => ({
            textLength: document.body.innerText.trim().length,
            childCount: document.getElementById("root")?.childElementCount ?? 0,
          }));
          assertCondition(
            pageErrors.length === errorsBefore,
            `Uncaught page errors: ${pageErrors.slice(errorsBefore).join("; ")}`,
          );
          assertCondition(
            metrics.childCount > 0 && metrics.textLength > 0,
            `Gallery view rendered empty content: ${JSON.stringify(metrics)}`,
          );
        },
      );
    }
  } finally {
    await context.close();
  }

  // Mobile smoke check on default gallery view
  const mobileContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
    reducedMotion: "reduce",
  });
  try {
    const page = await mobileContext.newPage();
    page.setDefaultTimeout(20_000);
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));

    await runCheck(
      "mobile 390x844: component gallery default view mounts with zero page errors",
      async () => {
        await page.goto(origin);
        await page.locator("main").first().waitFor();
        const textLength = await page.evaluate(
          () => document.body.innerText.trim().length,
        );
        assertCondition(
          pageErrors.length === 0,
          `Uncaught mobile page errors: ${pageErrors.join("; ")}`,
        );
        assertCondition(textLength > 0, "Gallery rendered empty body on mobile");
      },
    );
  } finally {
    await mobileContext.close();
  }

  const passedCount = checks.filter((c) => c.pass).length;
  const failedCount = checks.length - passedCount;
  console.log(`\n${passedCount} pass\n${failedCount} fail`);
  console.log(
    JSON.stringify(
      {
        suite: "component-gallery",
        passed: failedCount === 0,
        total: checks.length,
        passedCount,
        failedCount,
        checks,
      },
      null,
      2,
    ),
  );
  if (failedCount > 0) {
    process.exitCode = 1;
  }
} finally {
  await browser?.close();
  await server.close();
  await rm(tempDir, { recursive: true, force: true });
}
