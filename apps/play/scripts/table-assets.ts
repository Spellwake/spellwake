// The current scene eagerly loads these existing study assets, including alternate materials.
export const tableAssets = [
  "cards/lightning-bolt.png",
  "cards/magic-back.jpg",
  "cards/mountain-m11.png",
  "cards/spellwake-sleeve-midnight.svg",
  "cards/sleeve-shivan-dragon.jpg",
  "cards/magic-unlettered.png",
  ...["initiate", "adept", "mage", "archmage", "benefactor"].flatMap((tier) => [
    `cards/supporter-${tier}.svg`,
    `cards/supporter-${tier}-mask.svg`,
  ]),
  "cards/matte-plastic-normal.png",
  "cards/matte-plastic-roughness.png",
  ...[
    "llanowar-elves",
    "runeclaw-bear",
    "serra-angel",
    "chandra-nalaar",
    "crystal-ball",
    "pacifism",
    "krenko",
  ].map((card) => `cards/${card}.png`),
  "fonts/albertsans.ttf",
  "fonts/albertsans-OFL.txt",
  "cards/PROVENANCE.md",
] as const;
