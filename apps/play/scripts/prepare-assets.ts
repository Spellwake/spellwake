import { copyFile, cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { appAssets } from "./app-assets";
import { runtimeAssets } from "./runtime-assets";
import {
  appAssetSource,
  logoAssetSource,
  tableAssetSource,
} from "./release-asset-source";

// Release builds take only the pinned runtime asset package (logo, card back,
// and accepted application UI artwork) and omit local deck-builder fixtures.
const release = process.env.SPELLWAKE_RELEASE_ASSETS === "true";
const source = tableAssetSource();
const output = resolve(import.meta.dir, "../public");

await rm(output, { recursive: true, force: true });
await Promise.all(
  runtimeAssets.map(async (asset) => {
    const destination = resolve(output, asset);
    await mkdir(dirname(destination), { recursive: true });
    await copyFile(resolve(source, asset), destination);
  }),
);
if (release) {
  const packagedApp = appAssetSource();
  await Promise.all([
    copyFile(logoAssetSource(), resolve(output, "logo.svg")),
    ...appAssets.map(async (asset) => {
      const destination = resolve(output, asset);
      await mkdir(dirname(destination), { recursive: true });
      await copyFile(resolve(packagedApp, asset), destination);
    }),
  ]);
  console.info(
    `Prepared ${runtimeAssets.length + appAssets.length + 1} release application assets.`,
  );
  process.exit(0);
}
console.info(`Prepared ${runtimeAssets.length} runtime assets.`);

// Shell artwork is copied from the private asset working paths for local preview only.
// The backdrop is absent on purpose: its artwork is supplied separately.
// Those paths sit beside the code in the shared workspace. A checkout of the
// code alone has none of them, and names that workspace here instead.
const named = process.env.SPELLWAKE_PREVIEW_WORKSPACE;
const workspace = resolve(named ?? resolve(import.meta.dir, "../../.."));
const assetRoot = resolve(workspace, "assets");
const applicationAssets = [
  { from: logoAssetSource(), to: "logo.svg" },
  { from: resolve(assetRoot, "ui-study/coin.svg"), to: "coin.svg" },
  {
    from: resolve(assetRoot, "deck-box/models/deck-box-leather.glb"),
    to: "models/deck-box-leather.glb",
  },
  ...["color", "normal", "roughness"].map((map) => ({
    from: resolve(assetRoot, `table-scene/materials/leather-${map}.jpg`),
    to: `materials/leather-${map}.jpg`,
  })),
  ...["W", "U", "B", "R", "G", "C"].map((code) => ({
    from: resolve(assetRoot, `mana-symbols/${code}.svg`),
    to: `mana-symbols/${code}.svg`,
  })),
];
// Account entry: the approved provider marks and the two open-licensed typefaces its card is set in.
const entryReview = resolve(
  workspace,
  ".impeccable/review/open-play/account-entry",
);
const fonts = resolve(import.meta.dir, "../../control-gallery/fixtures/fonts");
applicationAssets.push(
  { from: resolve(entryReview, "google-symbol.svg"), to: "entry/google.svg" },
  { from: resolve(entryReview, "discord-symbol.svg"), to: "entry/discord.svg" },
  { from: resolve(fonts, "albert-sans.woff2"), to: "fonts/albert-sans.woff2" },
  {
    from: resolve(fonts, "albert-sans-OFL.txt"),
    to: "fonts/albert-sans-OFL.txt",
  },
  { from: resolve(fonts, "lora.woff2"), to: "fonts/lora.woff2" },
  { from: resolve(fonts, "lora-OFL.txt"), to: "fonts/lora-OFL.txt" },
);
// Account settings and profiles: the Patreon mark the owner supplied, and the rank emblems.
applicationAssets.push(
  {
    from: resolve(
      workspace,
      ".impeccable/review/open-play/account-settings/patreon-user-icon.svg",
    ),
    to: "account/patreon.svg",
  },
  ...[
    "unranked",
    "bronze",
    "silver",
    "gold",
    "platinum",
    "diamond",
    "mythic",
  ].map((tier) => ({
    from: resolve(assetRoot, `rank-icons/${tier}.svg`),
    to: `ranks/${tier}.svg`,
  })),
);
// The Apple mark is drawn in the page's ink, which an image cannot inherit, so it is written white.
await mkdir(resolve(output, "entry"), { recursive: true });
await writeFile(
  resolve(output, "entry/apple.svg"),
  (await readFile(resolve(entryReview, "apple-symbol.svg"), "utf8")).replace(
    "<path ",
    '<path fill="white" ',
  ),
);
await Promise.all(
  applicationAssets.map(async (asset) => {
    const destination = resolve(output, asset.to);
    await mkdir(dirname(destination), { recursive: true });
    await copyFile(asset.from, destination);
  }),
);
console.info(
  `Prepared ${applicationAssets.length} application preview assets.`,
);

// Deck builder preview cards: the review fixtures' real printings, art crops and mana symbols.
// They stand in for the catalogue service and are never part of a release build.
const fixtureRoot = resolve(
  workspace,
  "output/imagegen/guildhall-screen-set/components",
);
const builderOutput = resolve(output, "builder-preview");
type FixtureCard = { image: string; art_image: string } & Record<
  string,
  unknown
>;
const fixtureCards = (
  await Promise.all(
    ["assets/catalog.json", "assets/search-cards/catalog.json"].map(
      async (file) =>
        JSON.parse(
          await readFile(resolve(fixtureRoot, file), "utf8"),
        ) as FixtureCard[],
    ),
  )
).flat();
await mkdir(resolve(builderOutput, "cards"), { recursive: true });
await Promise.all(
  fixtureCards.flatMap((card) =>
    [card.image, card.art_image].map((image) =>
      copyFile(
        resolve(fixtureRoot, image),
        resolve(builderOutput, "cards", basename(image)),
      ),
    ),
  ),
);
await cp(resolve(fixtureRoot, "assets/mana"), resolve(builderOutput, "mana"), {
  recursive: true,
});
// Match preview: three original faces from the approved in-game design
// package, for the preview's two-mode, two-target and divided spells. Like the
// deck builder's fixtures they are never part of a release build.
const matchFaces = resolve(
  workspace,
  ".impeccable/review/open-play/in-game/arena-reference",
);
await mkdir(resolve(output, "match-preview"), { recursive: true });
await Promise.all(
  ["cryptic-command", "common-bond", "arc-lightning"].map((name) =>
    copyFile(
      resolve(matchFaces, `${name}-existing-card.jpg`),
      resolve(output, "match-preview", `${name}.jpg`),
    ),
  ),
);
// Official set symbols, as Scryfall serves them, for the pack wrappers.
await cp(resolve(assetRoot, "set-symbols"), resolve(output, "sets"), {
  recursive: true,
  filter: (path) => !path.endsWith(".ts") && !path.endsWith(".md"),
});
await writeFile(
  resolve(builderOutput, "catalog.json"),
  JSON.stringify(
    fixtureCards.map((card) => ({
      ...card,
      image: `cards/${basename(card.image)}`,
      art_image: `cards/${basename(card.art_image)}`,
    })),
  ),
);
console.info(`Prepared ${fixtureCards.length} deck builder preview cards.`);
