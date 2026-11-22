import { resolve } from "node:path";
import { loadEnv } from "vite";
import { z } from "zod";
import { parsePublicConfiguration } from "../src/lib/public-config";
import {
  nativeDevOrigin,
  nativeSecurityOverlay,
  withNativeOverlay,
} from "./native-config";

const root = resolve(import.meta.dir, "../../..");
/** One environment feeds Vite and its native CSP; no credentials enter command arguments. */
async function main(): Promise<number> {
  const environment = loadEnv("tauri", resolve(root, "apps/play"), "VITE_");
  const selected = parsePublicConfiguration(environment);
  if (selected.status === "invalid") {
    console.error(
      "Native build requires valid VITE_SUPABASE_URL, VITE_SUPABASE_PUBLISHABLE_KEY and VITE_APPLICATION_API_URL together, or none of them.",
    );
    return 1;
  }
  const base = z
    .object({
      app: z.object({ security: z.object({ csp: z.string().min(1) }) }),
    })
    .parse(await Bun.file(resolve(root, "src-tauri/tauri.conf.json")).json());
  const devOrigin = nativeDevOrigin(process.env["TAURI_DEV_HOST"]);
  const hmr = new URL(devOrigin);
  hmr.protocol = "ws:";
  if (process.env["TAURI_DEV_HOST"]) hmr.port = "6205";
  const overlay = nativeSecurityOverlay(
    base.app.security.csp,
    selected.status === "ready" ? selected.config : null,
    devOrigin,
    hmr.origin,
  );
  const child = Bun.spawn(
    [
      process.execPath,
      "x",
      "--no-install",
      "tauri",
      ...withNativeOverlay(process.argv.slice(2), overlay),
    ],
    {
      cwd: root,
      env: { ...process.env, ...environment },
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
    },
  );
  return child.exited;
}
if (import.meta.main) {
  try {
    process.exitCode = await main();
  } catch {
    console.error("Native configuration or build could not be started.");
    process.exitCode = 1;
  }
}
