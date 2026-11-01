import { mkdir, mkdtemp, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { tableAssets } from "./table-assets";
import { releaseAssetPackage } from "./release-asset-source";

const token = process.env.SPELLWAKE_ASSETS_TOKEN;
const assetPackage = await releaseAssetPackage();
const revision = assetPackage.revision;
if (!token || !revision || !/^[a-f0-9]{40}$/.test(revision)) {
  console.error(
    "Set SPELLWAKE_ASSETS_TOKEN and pin the manually exported runtime package commit in apps/play/release-assets.json.",
  );
  process.exit(1);
}
const root = resolve(import.meta.dir, "../../..");
const staging = await mkdtemp(join(root, ".release-assets-"));
const files = ["logo.svg", ...tableAssets.map((path) => `tabletop/${path}`)];
try {
  const results = await Promise.allSettled(
    files.map(async (path) => {
      const url = new URL(
        `https://api.github.com/repos/Spellwake/spellwake-assets/contents/${assetPackage.path}/${path}`,
      );
      url.searchParams.set("ref", revision);
      let response: Response;
      try {
        response = await fetch(url, {
          headers: {
            Authorization: `Bearer ${token}`,
            Accept: "application/vnd.github.raw+json",
            "X-GitHub-Api-Version": "2022-11-28",
          },
          signal: AbortSignal.timeout(60000),
        });
      } catch {
        throw new Error(`Could not download runtime asset: ${path}`);
      }
      if (!response.ok)
        throw new Error(
          `Runtime asset unavailable (${response.status}): ${path}`,
        );
      const destination = join(staging, path);
      await mkdir(dirname(destination), { recursive: true });
      await writeFile(
        destination,
        new Uint8Array(await response.arrayBuffer()),
      );
    }),
  );
  const failure = results.find((result) => result.status === "rejected");
  if (failure?.status === "rejected") {
    throw failure.reason instanceof Error
      ? failure.reason
      : new Error("Runtime asset download failed.");
  }
  const destination = join(root, ".release-assets");
  await rm(destination, { recursive: true, force: true });
  await rename(staging, destination);
  console.info(
    `Prepared ${files.length} runtime assets from pinned revision ${revision}.`,
  );
} catch (error: unknown) {
  console.error(
    error instanceof Error ? error.message : "Runtime asset staging failed.",
  );
  process.exitCode = 1;
} finally {
  await rm(staging, { recursive: true, force: true });
}
