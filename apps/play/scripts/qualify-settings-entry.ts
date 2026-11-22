/** Fast static-build entry contract before allocating a full managed qualification realm. */
import { chromium, expect } from "@playwright/test";
import { resolve } from "node:path";
const origin = process.argv[2];
if (!origin || new URL(origin).hostname !== "127.0.0.1")
  throw new Error("Owned loopback preview URL required.");
const binary = resolve("apps/play/node_modules/.bin/vite");
const processHandle = Bun.spawn(
  [
    binary,
    "preview",
    "--mode",
    "web",
    "--host",
    "127.0.0.1",
    "--port",
    new URL(origin).port,
    "--strictPort",
    "--outDir",
    process.argv[3] ?? "dist",
  ],
  { cwd: resolve("apps/play"), stdout: "ignore", stderr: "inherit" },
);
let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined;
try {
  for (let i = 0; i < 100; i++) {
    if (processHandle.exitCode !== null)
      throw new Error("Owned preview exited before readiness.");
    try {
      if ((await fetch(origin, { signal: AbortSignal.timeout(500) })).ok) break;
    } catch {
      /* Bounded local readiness retry. */
    }
    if (i === 99) throw new Error("Owned preview readiness failed.");
    await Bun.sleep(100);
  }
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 390, height: 844 } });
  await page.goto(origin + "/collection");
  await page
    .getByRole("button", { name: "Open account menu", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Default settings", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Account settings", exact: true }),
  ).toBeVisible();
  await expect(
    page
      .getByRole("dialog", { name: "Account settings", exact: true })
      .getByText("Sign-in is unavailable right now.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  console.log(
    JSON.stringify({
      passed: true,
      checks: [
        "actual shared menu entry opens account settings",
        "unconfigured state closes without a server request",
      ],
    }),
  );
} finally {
  await browser?.close();
  processHandle.kill("SIGTERM");
  await processHandle.exited;
}
