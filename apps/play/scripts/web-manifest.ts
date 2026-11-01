import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { releaseAssetRevision } from "./release-asset-source";

const root = resolve(import.meta.dir, "../../..");
const commit = process.env.RENDER_GIT_COMMIT ?? process.env.GITHUB_SHA ?? null;
if (commit !== null && !/^[a-f0-9]{40}$/.test(commit)) {
  console.error("Web release revision must be a full Git commit hash.");
  process.exit(1);
}
const assetRevision =
  process.env.SPELLWAKE_RELEASE_ASSETS === "true"
    ? await releaseAssetRevision()
    : null;
if (process.env.SPELLWAKE_RELEASE_ASSETS === "true" && !assetRevision) {
  console.error(
    "The release asset revision must be pinned before building the web release.",
  );
  process.exit(1);
}
const config: { version: string } = await Bun.file(
  resolve(root, "src-tauri/tauri.conf.json"),
).json();
await writeFile(
  resolve(root, "apps/play/dist/release.json"),
  JSON.stringify({ version: config.version, commit, assetRevision }),
);
