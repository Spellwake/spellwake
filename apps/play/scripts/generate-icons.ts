import { logoAssetSource } from "./release-asset-source";
import { existsSync } from "node:fs";
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dir, "../../..");
const logo = await readFile(logoAssetSource(), "utf8");
const directory = await mkdtemp(join(tmpdir(), "spellwake-icon-"));

// Compose the native tile around the canonical transparent logo without forking its paths.
const artwork = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <title>Spellwake app icon</title>
  <defs>
    <linearGradient id="app-charcoal" x1="0" y1="0" x2="0.25" y2="1">
      <stop stop-color="#303135"/>
      <stop offset="1" stop-color="#151619"/>
    </linearGradient>
  </defs>
  <rect x="96" y="96" width="832" height="832" rx="186" fill="url(#app-charcoal)"/>
  <g transform="translate(238 192) scale(1.8266666667)">
    ${logo}
  </g>
</svg>`;

try {
  const input = join(directory, "app-icon.svg");
  await writeFile(input, artwork);
  const result = Bun.spawnSync(
    ["bun", "run", "tauri", "icon", input, "--output", "src-tauri/icons"],
    { cwd: root, stdout: "inherit", stderr: "inherit" },
  );
  if (result.exitCode !== 0) process.exit(result.exitCode);

  // Mobile launchers supply their own shape; use an opaque full-bleed background.
  const tile =
    '<rect x="96" y="96" width="832" height="832" rx="186" fill="url(#app-charcoal)"/>';
  const mobile = artwork.replace(
    tile,
    '<rect width="1024" height="1024" fill="url(#app-charcoal)"/>',
  );
  const foreground = artwork.replace(tile, "");
  const background = mobile.replace(
    /<g transform=[\s\S]*?<\/g>\s*<\/svg>$/,
    "</svg>",
  );
  await Promise.all([
    writeFile(join(directory, "mobile.svg"), mobile),
    writeFile(join(directory, "foreground.svg"), foreground),
    writeFile(join(directory, "background.svg"), background),
    writeFile(
      join(directory, "mobile.json"),
      JSON.stringify({
        default: "mobile.svg",
        bg_color: "#202124",
        android_bg: "background.svg",
        android_fg: "foreground.svg",
        android_fg_scale: 85,
      }),
    ),
  ]);
  const mobileOutput = join(directory, "mobile");
  const mobileResult = Bun.spawnSync(
    [
      "bun",
      "run",
      "tauri",
      "icon",
      join(directory, "mobile.json"),
      "--output",
      mobileOutput,
    ],
    { cwd: root, stdout: "inherit", stderr: "inherit" },
  );
  if (mobileResult.exitCode !== 0) process.exit(mobileResult.exitCode);
  await Promise.all(
    ["ios", "android"].map((platform) =>
      cp(
        join(mobileOutput, platform),
        join(root, "src-tauri/icons", platform),
        { recursive: true },
      ),
    ),
  );
  for (const [platform, destination] of [
    ["ios", "src-tauri/gen/apple/Assets.xcassets/AppIcon.appiconset"],
    ["android", "src-tauri/gen/android/app/src/main/res"],
  ]) {
    if (platform && destination && existsSync(join(root, destination))) {
      await cp(join(mobileOutput, platform), join(root, destination), {
        recursive: true,
      });
    }
  }
} finally {
  await rm(directory, { recursive: true, force: true });
}
