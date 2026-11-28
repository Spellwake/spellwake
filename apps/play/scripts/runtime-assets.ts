/** Exact tabletop artwork shipped from the pinned runtime asset package. */
export const runtimeAssets = [
  "cards/spellwake-sleeve-midnight.svg",
  "cards/supporter-initiate.svg",
  "cards/supporter-initiate-mask.svg",
  "cards/supporter-adept.svg",
  "cards/supporter-adept-mask.svg",
  "cards/supporter-mage.svg",
  "cards/supporter-mage-mask.svg",
  "cards/supporter-archmage.svg",
  "cards/supporter-archmage-mask.svg",
  "cards/supporter-benefactor.svg",
  "cards/supporter-benefactor-mask.svg",
] as const;

/**
 * Approved raster playmat artwork exported to `Spellwake/spellwake-assets`
 * under `approved/app-runtime/tabletop/playmats/`. Append to `runtimeAssets`
 * after committing the asset export and pinning the new revision in
 * `apps/play/release-assets.json`.
 */
export const playmatRuntimeAssets = [
  "playmats/windswept-castle.jpg",
  "playmats/dragon-and-tiger.jpg",
  "playmats/twilight-in-the-wilderness.jpg",
  "playmats/rocky-wooded-landscape.jpg",
  "playmats/landscape-with-castle-and-shepherds.jpg",
  "playmats/landscape-with-a-windmill.jpg",
  "playmats/water-lily-pond.jpg",
  "playmats/branch-hill-pond.jpg",
  "playmats/under-the-wave-off-kanagawa.jpg",
  "playmats/marine.jpg",
  "playmats/boats-at-berck-sur-mer.jpg",
  "playmats/mount-vesuvius-at-midnight.jpg",
  "playmats/burning-of-houses-of-parliament.jpg",
  "playmats/rocky-mountains-landers-peak.jpg",
  "playmats/northeaster.jpg",
  "playmats/tiger-in-bamboo.jpg",
  "playmats/barks-fleeing-before-the-storm.jpg",
  "playmats/cliff-walk-at-pourville.jpg",
  "playmats/the-poets-garden.jpg",
  "playmats/poppy-field-giverny.jpg",
  "playmats/the-petite-creuse-river.jpg",
] as const;
