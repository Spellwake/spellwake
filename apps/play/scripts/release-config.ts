import { appendFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dir, "../../..");
const run = Number(process.env.GITHUB_RUN_NUMBER ?? "1");
const attempt = Number(process.env.GITHUB_RUN_ATTEMPT ?? "1");
const buildNumber = run * 100 + attempt;
if (
  !Number.isSafeInteger(run) ||
  run < 1 ||
  !Number.isInteger(attempt) ||
  attempt < 1 ||
  attempt > 99 ||
  !Number.isSafeInteger(buildNumber) ||
  buildNumber < 1 ||
  buildNumber > 2100000000
) {
  console.error("Release build number is outside the supported store range.");
  process.exit(1);
}
// Keep the marketing version canonical in tauri.conf.json; vary only the store build number.
await writeFile(
  resolve(root, "src-tauri/release.generated.json"),
  JSON.stringify(
    {
      bundle: {
        android: { versionCode: buildNumber },
        iOS: { bundleVersion: String(buildNumber) },
        macOS: { bundleVersion: String(buildNumber) },
      },
    },
    null,
    2,
  ),
);

const output = process.env.GITHUB_OUTPUT;
if (output) {
  const config: { version: string } = await Bun.file(
    resolve(root, "src-tauri/tauri.conf.json"),
  ).json();
  if (!/^\d+\.\d+\.\d+$/.test(config.version)) {
    console.error(
      "Release version must use the shared numeric major.minor.patch format.",
    );
    process.exit(1);
  }
  await appendFile(output, `version=${config.version}\n`);
}
