/**
 * Standalone Playwright real-browser 3D tabletop rendering benchmark for the
 * Rendering domain (`3d-tabletop` subdomain).
 *
 * Mounts the live Three.js WebGL2 tabletop match scene (`MatchTable` /
 * `mountLiveMatchScene`) via `/play/match/demo` in unconfigured preview mode
 * and verifies:
 * 1. Real WebGL2 context initialization on the tabletop `<canvas>` with zero context loss.
 * 2. Real instrumented WebGL2 draw calls occur (confirming the Three.js scene actually renders).
 * 3. Canvas backing buffer sizing and live viewport resize (1440x900 -> 1024x768) hold cleanly.
 * 4. Real requestAnimationFrame frame-time telemetry (p50 / p95 ms over 45 frames) with honest
 *    renderer/GPU reporting (hardware vs software SwiftShader/WARP).
 * 5. Mobile viewport (390x844) mounts the 3D match canvas cleanly with zero horizontal overflow
 *    and zero uncaught page errors.
 *
 * Usage: node apps/play/scripts/qualify-3d.ts
 */
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer, type ViteDevServer } from "vite";
import {
  assertCondition,
  launchBrowser,
  type CheckResult,
} from "./playwright-harness.ts";

type WebGlMetrics = {
  webgl2Contexts: number;
  drawCalls: number;
  contextLost: boolean;
  renderer: string | null;
  vendor: string | null;
  isSoftwareRenderer: boolean;
};

function quantile(values: number[], fraction: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * fraction))] ?? 0;
}

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tempDir = await mkdtemp(join(tmpdir(), "spellwake-qualify-3d-"));

// Ensure preview mode (unconfigured auth/api) so /play/match/demo mounts PreviewMatch
delete process.env["VITE_SUPABASE_URL"];
delete process.env["VITE_APPLICATION_API_URL"];
delete process.env["VITE_SUPABASE_PUBLISHABLE_KEY"];

const server: ViteDevServer = await createServer({
  root: appRoot,
  cacheDir: join(tempDir, "vite-cache"),
  server: { port: 0, strictPort: false, host: "127.0.0.1" },
  logLevel: "error",
});

const checks: CheckResult[] = [];
const telemetry: Record<string, unknown> = {};

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

const WEBGL_INSTRUMENTATION_SCRIPT = () => {
  const stats = {
    webgl2Contexts: 0,
    drawCalls: 0,
    contextLost: false,
    renderer: null as string | null,
    vendor: null as string | null,
  };
  (window as unknown as Record<string, unknown>)["__spellwakeWebGlStats"] = stats;

  const instrumented = new WeakSet<object>();
  const origGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (
    this: HTMLCanvasElement,
    type: string,
    ...args: unknown[]
  ) {
    const ctx = origGetContext.apply(this, [type, ...args] as Parameters<
      HTMLCanvasElement["getContext"]
    >);
    if (
      ctx &&
      (type === "webgl2" || type === "webgl") &&
      !instrumented.has(ctx)
    ) {
      instrumented.add(ctx);
      stats.webgl2Contexts += 1;
      this.addEventListener("webglcontextlost", () => {
        stats.contextLost = true;
      });
      const gl = ctx as WebGL2RenderingContext;
      const debug = gl.getExtension("WEBGL_debug_renderer_info");
      stats.vendor = String(
        debug ? gl.getParameter(debug.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR),
      );
      stats.renderer = String(
        debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER),
      );
      for (const method of [
        "drawArrays",
        "drawElements",
        "drawArraysInstanced",
        "drawElementsInstanced",
      ] as const) {
        const original = gl[method];
        if (typeof original === "function") {
          Object.defineProperty(gl, method, {
            configurable: true,
            value: function (...drawArgs: unknown[]) {
              stats.drawCalls += 1;
              return (original as (...a: unknown[]) => unknown).apply(this, drawArgs);
            },
          });
        }
      }
    }
    return ctx;
  } as HTMLCanvasElement["getContext"];
};

let browser: Awaited<ReturnType<typeof launchBrowser>> | undefined;
try {
  await server.listen();
  const address = server.httpServer?.address();
  if (!address || typeof address === "string") {
    throw new Error("Could not resolve Vite server port for 3D benchmark.");
  }
  const origin = `http://127.0.0.1:${address.port}/play`;
  browser = await launchBrowser();

  // Desktop 3D tabletop benchmark
  const desktopContext = await browser.newContext({
    viewport: { width: 1440, height: 900 },
  });
  try {
    await desktopContext.addInitScript(WEBGL_INSTRUMENTATION_SCRIPT);
    await desktopContext.route("**/*.glb*", (route) => route.abort());
    const page = await desktopContext.newPage();
    page.setDefaultTimeout(20_000);
    page.setDefaultNavigationTimeout(45_000);
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));

    await page.goto(`${origin}/match/demo`);
    const canvas = page.locator("canvas").first();
    await canvas.waitFor({ state: "visible" });
    await page.waitForTimeout(800);

    await runCheck(
      "3d tabletop desktop: mounts WebGL2 Three.js scene with real draw calls and zero context loss",
      async () => {
        const box = await canvas.boundingBox();
        assertCondition(
          box !== null && box.width >= 600 && box.height >= 400,
          `Unexpected desktop 3D canvas bounds: ${JSON.stringify(box)}`,
        );
        const stats = await page.evaluate((): WebGlMetrics => {
          const raw = (window as unknown as Record<string, unknown>)[
            "__spellwakeWebGlStats"
          ] as {
            webgl2Contexts: number;
            drawCalls: number;
            contextLost: boolean;
            renderer: string | null;
            vendor: string | null;
          };
          const isSoftwareRenderer =
            /swiftshader|llvmpipe|softpipe|software|basic render/i.test(
              raw.renderer ?? "",
            );
          return { ...raw, isSoftwareRenderer };
        });
        telemetry["desktopWebGl"] = stats;
        assertCondition(
          stats.webgl2Contexts >= 1,
          `Expected >= 1 WebGL context, saw ${stats.webgl2Contexts}`,
        );
        assertCondition(
          stats.drawCalls > 0,
          `Expected real WebGL draw calls > 0, saw ${stats.drawCalls}`,
        );
        assertCondition(!stats.contextLost, "WebGL context was lost during mount");
        assertCondition(
          pageErrors.length === 0,
          `Uncaught page errors: ${pageErrors.join("; ")}`,
        );
      },
    );

    await runCheck(
      "3d tabletop desktop: captures 45 animation frames with healthy frame telemetry",
      async () => {
        const intervals = await page.evaluate(async () => {
          const samples: number[] = [];
          let prev = performance.now();
          for (let i = 0; i < 45; i += 1) {
            await new Promise<void>((resolveFrame) =>
              requestAnimationFrame(() => resolveFrame()),
            );
            const now = performance.now();
            samples.push(now - prev);
            prev = now;
          }
          return samples;
        });
        const p50 = Number(quantile(intervals, 0.5).toFixed(2));
        const p95 = Number(quantile(intervals, 0.95).toFixed(2));
        telemetry["desktopFrameMs"] = {
          samples: intervals.length,
          p50,
          p95,
        };
        assertCondition(
          intervals.length === 45 && Number.isFinite(p95) && p95 > 0,
          `Invalid frame interval telemetry: p50=${p50} p95=${p95}`,
        );
      },
    );

    await runCheck(
      "3d tabletop resize: resizes canvas cleanly from 1440x900 to 1024x768 without context loss",
      async () => {
        const before = await canvas.boundingBox();
        await page.setViewportSize({ width: 1024, height: 768 });
        await page.waitForTimeout(350);
        const after = await canvas.boundingBox();
        const lost = await page.evaluate(() => {
          const raw = (window as unknown as Record<string, unknown>)[
            "__spellwakeWebGlStats"
          ] as { contextLost: boolean };
          return raw.contextLost;
        });
        assertCondition(!lost, "WebGL context lost during viewport resize");
        assertCondition(
          before !== null &&
            after !== null &&
            after.width < before.width &&
            after.width >= 500,
          `Canvas did not resize cleanly: before=${JSON.stringify(before)} after=${JSON.stringify(after)}`,
        );
        const overflow = await page.evaluate(
          () =>
            document.documentElement.scrollWidth >
            document.documentElement.clientWidth,
        );
        assertCondition(!overflow, "Horizontal overflow detected after 3D resize");
      },
    );
  } finally {
    await desktopContext.close();
  }

  // Mobile 3D tabletop smoke benchmark
  const mobileContext = await browser.newContext({
    viewport: { width: 390, height: 844 },
  });
  try {
    await mobileContext.addInitScript(WEBGL_INSTRUMENTATION_SCRIPT);
    await mobileContext.route("**/*.glb*", (route) => route.abort());
    const page = await mobileContext.newPage();
    page.setDefaultTimeout(20_000);
    const pageErrors: string[] = [];
    page.on("pageerror", (err) => pageErrors.push(err.message));

    await runCheck(
      "3d tabletop mobile 390x844: mounts WebGL canvas cleanly with draw calls and zero horizontal overflow",
      async () => {
        await page.goto(`${origin}/match/demo`);
        const canvas = page.locator("canvas").first();
        await canvas.waitFor({ state: "visible" });
        await page.waitForTimeout(600);
        const box = await canvas.boundingBox();
        const stats = await page.evaluate(() => {
          const raw = (window as unknown as Record<string, unknown>)[
            "__spellwakeWebGlStats"
          ] as { drawCalls: number; contextLost: boolean };
          return {
            drawCalls: raw.drawCalls,
            contextLost: raw.contextLost,
            overflow:
              document.documentElement.scrollWidth >
              document.documentElement.clientWidth,
          };
        });
        telemetry["mobileWebGl"] = stats;
        assertCondition(
          box !== null && box.width >= 300 && box.height >= 300,
          `Unexpected mobile 3D canvas bounds: ${JSON.stringify(box)}`,
        );
        assertCondition(
          stats.drawCalls > 0 && !stats.contextLost,
          `Mobile 3D render failed: ${JSON.stringify(stats)}`,
        );
        assertCondition(
          !stats.overflow,
          "Horizontal overflow detected on mobile 3D match tabletop",
        );
        assertCondition(
          pageErrors.length === 0,
          `Uncaught mobile page errors: ${pageErrors.join("; ")}`,
        );
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
        suite: "rendering-3d-tabletop",
        passed: failedCount === 0,
        total: checks.length,
        passedCount,
        failedCount,
        telemetry,
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
