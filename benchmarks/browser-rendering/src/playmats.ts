/**
 * Canonical 53-playmat catalogue and deterministic 2D/3D playmat renderer.
 *
 * Covers three approved lines (zero free starter playmats; all priced $0.99 to $3.99):
 * 1. Brand & Ornamental (18): Official Spellwake logo SVGs with color-matched merrowed
 *    edges or clean stitchless edges, zero wordmark, zero fixed card zones.
 * 2. Tactile Materials (14): Procedural felts, neoprene, marble, basalt, saddle-stitched
 *    framed leathers, linen, vellum, indigo canvas, and washi paper.
 * 3. Locked Artwork (21): Commissioned Windswept Castle plus 20 verified CC0 public-domain
 *    museum paintings served from the pinned Spellwake asset package (`playmats/<slug>.jpg`).
 *
 * Surface texture overlays are intentionally disabled across all playmats.
 */
import * as THREE from "three";
import {
  paintPlaymatEffectOverlay,
  type PlaymatEffectSelection,
} from "./playmat-effects";

export type PlaymatGroup = "brand" | "material" | "art";
export type PlaymatRarity = "common" | "uncommon" | "rare" | "mythic";

export type ThreadPalette = {
  readonly base: readonly [number, number, number];
  readonly hi: readonly [number, number, number];
  readonly lo: readonly [number, number, number];
  readonly crevice: string;
  readonly lockCord: string;
};

export type PlaymatEdgeStyle =
  | { readonly kind: "none"; readonly label: string }
  | {
      readonly kind: "merrowed";
      readonly key: string;
      readonly label: string;
      readonly palette: ThreadPalette;
    }
  | {
      readonly kind: "saddle";
      readonly key: string;
      readonly label: string;
      readonly thread: string;
      readonly groove: string;
      readonly trim?: string;
    };

export type PlaymatSpec = {
  readonly slug: string;
  readonly canonicalId: string;
  readonly previewId: string;
  readonly assetRef: string;
  readonly name: string;
  readonly group: PlaymatGroup;
  readonly subtitle: string;
  readonly description: string;
  readonly priceCents: 99 | 199 | 299 | 399;
  readonly priceLabel: "$0.99" | "$1.99" | "$2.99" | "$3.99";
  readonly rarity: PlaymatRarity;
  readonly edge: PlaymatEdgeStyle;
  /** Relative path under the app base URL for raster art playmats (`playmats/<slug>.jpg`). */
  readonly artAssetPath?: string;
  /** Attribution metadata for art playmats. */
  readonly attribution?: {
    readonly artist: string;
    readonly date: string;
    readonly museum: string;
    readonly license: string;
  };
};

function makeUuid(seq: number): string {
  return `e0000003-0001-4000-8000-${String(seq).padStart(12, "0")}`;
}

function priceLabelOf(
  cents: 99 | 199 | 299 | 399,
): "$0.99" | "$1.99" | "$2.99" | "$3.99" {
  switch (cents) {
    case 99:
      return "$0.99";
    case 199:
      return "$1.99";
    case 299:
      return "$2.99";
    case 399:
      return "$3.99";
  }
}

function rarityOf(cents: 99 | 199 | 299 | 399): PlaymatRarity {
  switch (cents) {
    case 99:
      return "common";
    case 199:
      return "uncommon";
    case 299:
      return "rare";
    case 399:
      return "mythic";
  }
}

function makeThreadPalette(
  base: readonly [number, number, number],
  hiLift = 44,
  loDrop = 58,
): ThreadPalette {
  const clamp = (n: number) => Math.max(0, Math.min(255, Math.round(n)));
  return {
    base,
    hi: [
      clamp(base[0] + hiLift),
      clamp(base[1] + hiLift),
      clamp(base[2] + hiLift),
    ],
    lo: [
      clamp(base[0] - loDrop),
      clamp(base[1] - loDrop),
      clamp(base[2] - loDrop),
    ],
    crevice: `rgba(${clamp(base[0] * 0.16)},${clamp(base[1] * 0.16)},${clamp(base[2] * 0.16)},0.82)`,
    lockCord: `rgba(${clamp(base[0] * 0.85)},${clamp(base[1] * 0.85)},${clamp(base[2] * 0.85)},0.52)`,
  };
}

export const PLAYMAT_EDGES = {
  none: { kind: "none", label: "Clean stitchless edge" },
  dark: {
    kind: "merrowed",
    key: "dark",
    label: "Black merrowed stitch",
    palette: {
      base: [38, 41, 48],
      hi: [104, 111, 124],
      lo: [15, 17, 21],
      crevice: "rgba(4, 5, 8, 0.88)",
      lockCord: "rgba(74, 80, 92, 0.48)",
    },
  },
  light: {
    kind: "merrowed",
    key: "light",
    label: "Light cream merrowed stitch",
    palette: {
      base: [224, 214, 194],
      hi: [252, 247, 236],
      lo: [148, 137, 118],
      crevice: "rgba(30, 26, 20, 0.72)",
      lockCord: "rgba(182, 170, 148, 0.55)",
    },
  },
  gold: {
    kind: "merrowed",
    key: "gold",
    label: "Gold merrowed stitch",
    palette: {
      base: [198, 162, 92],
      hi: [248, 224, 162],
      lo: [118, 88, 36],
      crevice: "rgba(34, 24, 9, 0.78)",
      lockCord: "rgba(176, 140, 72, 0.56)",
    },
  },
} as const satisfies Record<string, PlaymatEdgeStyle>;

function customMerrowed(
  key: string,
  label: string,
  base: readonly [number, number, number],
  hiLift = 46,
  loDrop = 56,
): PlaymatEdgeStyle {
  return {
    kind: "merrowed",
    key,
    label,
    palette: makeThreadPalette(base, hiLift, loDrop),
  };
}

function makeBrandEntry(
  seq: number,
  slug: string,
  name: string,
  subtitle: string,
  priceCents: 99 | 199 | 299 | 399,
  edge: PlaymatEdgeStyle,
): PlaymatSpec {
  return {
    slug,
    canonicalId: makeUuid(seq),
    previewId: `preview-playmat-${slug}`,
    assetRef: `playmat:brand:${slug}`,
    name,
    group: "brand",
    subtitle,
    description: `${subtitle}.`,
    priceCents,
    priceLabel: priceLabelOf(priceCents),
    rarity: rarityOf(priceCents),
    edge,
  };
}

function makeMaterialEntry(
  seq: number,
  slug: string,
  name: string,
  subtitle: string,
  priceCents: 99 | 199 | 299 | 399,
  edge: PlaymatEdgeStyle,
): PlaymatSpec {
  return {
    slug,
    canonicalId: makeUuid(seq),
    previewId: `preview-playmat-${slug}`,
    assetRef: `playmat:material:${slug}`,
    name,
    group: "material",
    subtitle,
    description: `${subtitle}.`,
    priceCents,
    priceLabel: priceLabelOf(priceCents),
    rarity: rarityOf(priceCents),
    edge,
  };
}

function makeArtEntry(
  seq: number,
  slug: string,
  name: string,
  artist: string,
  date: string,
  museum: string,
  license: string,
  priceCents: 99 | 199 | 299 | 399,
  stitch: "dark" | "light" | "gold",
): PlaymatSpec {
  const edge = PLAYMAT_EDGES[stitch];
  return {
    slug,
    canonicalId: makeUuid(seq),
    previewId: `preview-playmat-${slug}`,
    assetRef: `playmat:art:${slug}`,
    name,
    group: "art",
    subtitle: `${artist} (${date})`,
    description: `${name} by ${artist} (${date}), ${museum}.`,
    priceCents,
    priceLabel: priceLabelOf(priceCents),
    rarity: rarityOf(priceCents),
    edge,
    artAssetPath: `playmats/${slug}.jpg`,
    attribution: { artist, date, museum, license },
  };
}

/** 18 approved Spellwake Brand & Ornamental playmats ($1.99 to $2.99). */
export const BRAND_PLAYMATS: readonly PlaymatSpec[] = [
  makeBrandEntry(
    1,
    "obsidian-and-brass",
    "Obsidian & Brass",
    "Matte black field with inset brass filigree and gold crest",
    299,
    PLAYMAT_EDGES.gold,
  ),
  makeBrandEntry(
    2,
    "slate-filigree",
    "Slate Filigree",
    "High-contrast slate tabletop with silver framing and steel-blue stitch",
    199,
    customMerrowed("slate-blue", "Steel-blue merrowed stitch", [122, 144, 174], 50, 56),
  ),
  makeBrandEntry(
    3,
    "astral-compass",
    "Astral Compass",
    "Midnight navy compass dial with gilded Spellwake emblem",
    299,
    PLAYMAT_EDGES.gold,
  ),
  makeBrandEntry(
    4,
    "archivists-vellum",
    "Archivist's Vellum",
    "Warm parchment field with dark sepia frame and espresso-sepia stitch",
    199,
    customMerrowed("sepia-umber", "Sepia-umber merrowed stitch", [94, 74, 52], 42, 46),
  ),
  makeBrandEntry(
    5,
    "arcane-orrery",
    "Arcane Orrery",
    "Clean stitchless midnight astrolabe rings around gilded emblem",
    299,
    PLAYMAT_EDGES.none,
  ),
  makeBrandEntry(
    6,
    "fivefold-mana-star",
    "Fivefold Mana Star",
    "Clockwise five-color mana wheel with compact centered emblem",
    299,
    PLAYMAT_EDGES.dark,
  ),
  makeBrandEntry(
    7,
    "gilded-sunburst",
    "Gilded Sunburst",
    "Art-deco radiant brass rays on deep forest green with gold stitch",
    299,
    PLAYMAT_EDGES.gold,
  ),
  makeBrandEntry(
    8,
    "crimson-sanctum",
    "Crimson Sanctum",
    "Deep oxblood gothic arch with tonal crimson merrowed edge",
    199,
    customMerrowed("crimson-oxblood", "Crimson-oxblood merrowed stitch", [122, 36, 46], 46, 54),
  ),
  makeBrandEntry(
    9,
    "verdant-leylines",
    "Verdant Leylines",
    "Forest pine topographic contours with forest-moss stitch",
    199,
    customMerrowed("forest-moss", "Forest-moss merrowed stitch", [64, 114, 78], 44, 44),
  ),
  makeBrandEntry(
    10,
    "celestial-meridian",
    "Celestial Meridian",
    "Boundless stitchless starfield chart with ecliptic arc and silver crest",
    299,
    PLAYMAT_EDGES.none,
  ),
  makeBrandEntry(
    11,
    "sunken-foundry",
    "Sunken Foundry",
    "Bronze architectural blueprint grid with burnished-copper stitch",
    199,
    customMerrowed("copper-bronze", "Burnished-copper merrowed stitch", [176, 114, 68], 48, 56),
  ),
  makeBrandEntry(
    12,
    "alchemists-circle",
    "Alchemist's Circle",
    "Transmutation sigil geometry on plum-black with amethyst stitch",
    299,
    customMerrowed("amethyst-plum", "Amethyst-plum merrowed stitch", [148, 112, 178], 48, 58),
  ),
  makeBrandEntry(
    13,
    "ivory-cathedral",
    "Ivory Cathedral",
    "Inverted charcoal medallion and ivory crest inside a rose star",
    199,
    PLAYMAT_EDGES.dark,
  ),
  makeBrandEntry(
    14,
    "tidal-cartography",
    "Tidal Cartography",
    "Nautical rhumb lines on deep sea-teal with sea-teal stitch",
    199,
    customMerrowed("sea-teal", "Sea-teal merrowed stitch", [74, 152, 148], 48, 52),
  ),
  makeBrandEntry(
    15,
    "ember-crucible",
    "Ember Crucible",
    "Volcanic slate hexagonal mesh with smoldering-ember stitch",
    199,
    customMerrowed("ember-rust", "Ember-rust merrowed stitch", [198, 98, 54], 50, 62),
  ),
  makeBrandEntry(
    16,
    "moonphase-archive",
    "Moonphase Archive",
    "Symmetrical lunar phases with moonlight-silver emblem and stitch",
    299,
    customMerrowed("moon-silver", "Moonlight-silver merrowed stitch", [168, 182, 212], 50, 58),
  ),
  makeBrandEntry(
    17,
    "heraldic-diamond",
    "Heraldic Diamond",
    "Subtle royal harlequin lattice with gilded crest and gold stitch",
    299,
    PLAYMAT_EDGES.gold,
  ),
  makeBrandEntry(
    18,
    "monolith-minimal",
    "Monolith Minimal",
    "Ultra-clean stitchless tournament charcoal with corner emblem",
    199,
    PLAYMAT_EDGES.none,
  ),
];

/** 14 approved Tactile Material playmats ($0.99 to $1.99). */
export const MATERIAL_PLAYMATS: readonly PlaymatSpec[] = [
  makeMaterialEntry(
    19,
    "classic-casino-felt",
    "Classic Casino Felt",
    "Emerald-pine woven baize tabletop with clean stitchless edge",
    99,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    20,
    "burgundy-billiard-baize",
    "Burgundy Billiard Baize",
    "Deep wine-crimson wool felt with clean stitchless edge",
    99,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    21,
    "midnight-matte-neoprene",
    "Midnight Matte Neoprene",
    "Ultra-fine tournament navy jersey weave with clean stitchless edge",
    99,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    22,
    "charcoal-heather-neoprene",
    "Charcoal Heather Neoprene",
    "Speckled dark heather knit for crisp card contrast",
    99,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    23,
    "obsidian-and-gold-marble",
    "Obsidian & Gold Marble",
    "Nero Marquina black marble slab with gilded mineral veins",
    199,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    24,
    "carrara-white-marble",
    "Carrara White Marble",
    "Luminous pale Italian marble slab with misty gray veining",
    199,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    25,
    "basalt-flagstone",
    "Basalt Flagstone",
    "Matte dark volcanic stone slab with subtle mineral speckle",
    199,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    26,
    "saddle-espresso-leather",
    "Saddle Espresso Leather",
    "Pebble-grain brown leather with debossed crease and tan saddle-stitch",
    199,
    {
      kind: "saddle",
      key: "saddle-espresso",
      label: "Tan leather saddle-stitch and crease",
      thread: "#c99a63",
      groove: "rgba(18, 10, 5, 0.74)",
    },
  ),
  makeMaterialEntry(
    27,
    "oxblood-library-leather",
    "Oxblood Library Leather",
    "Oiled oxblood desk blotter with gold-tooled rule and saddle-stitch",
    199,
    {
      kind: "saddle",
      key: "saddle-oxblood",
      label: "Gold-tooled library leather framing",
      thread: "#b58b52",
      groove: "rgba(22, 6, 7, 0.78)",
      trim: "rgba(212, 174, 104, 0.48)",
    },
  ),
  makeMaterialEntry(
    28,
    "obsidian-grain-leather",
    "Obsidian Grain Leather",
    "Matte black pebble leather with blind-debossed frame and slate saddle-stitch",
    199,
    {
      kind: "saddle",
      key: "saddle-obsidian",
      label: "Blind-debossed and slate saddle-stitch frame",
      thread: "#78808f",
      groove: "rgba(4, 5, 8, 0.85)",
      trim: "rgba(154, 164, 182, 0.22)",
    },
  ),
  makeMaterialEntry(
    29,
    "woven-natural-linen",
    "Woven Natural Linen",
    "Warm oatmeal cross-hatch cloth with dark espresso merrowed edge",
    99,
    customMerrowed("linen-espresso", "Espresso-charcoal merrowed stitch", [48, 42, 36], 44, 34),
  ),
  makeMaterialEntry(
    30,
    "cartographers-vellum",
    "Cartographer's Vellum",
    "Tea-stained antique parchment with clean unhemmed paper edge",
    99,
    PLAYMAT_EDGES.none,
  ),
  makeMaterialEntry(
    31,
    "raw-indigo-canvas",
    "Raw Indigo Canvas",
    "Heavyweight Japanese indigo duck cloth with ecru selvedge stitch",
    99,
    customMerrowed("indigo-ecru", "Ecru selvedge merrowed stitch", [208, 196, 174], 40, 56),
  ),
  makeMaterialEntry(
    32,
    "charcoal-washi-paper",
    "Charcoal Washi Paper",
    "Handmade dark mulberry paper with delicate organic fibers",
    99,
    PLAYMAT_EDGES.none,
  ),
];

/** 21 locked Artwork playmats (1 commissioned flagship + 20 verified CC0 paintings, $2.99 to $3.99). */
export const ART_PLAYMATS: readonly PlaymatSpec[] = [
  makeArtEntry(
    33,
    "windswept-castle",
    "Windswept Castle",
    "Andreas Rocha",
    "2025",
    "Commissioned Flagship",
    "Owned Commercial Rights",
    399,
    "dark",
  ),
  makeArtEntry(
    34,
    "dragon-and-tiger",
    "Dragon and Tiger",
    "Sesson Shūkei",
    "c. 1546-56",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    399,
    "dark",
  ),
  makeArtEntry(
    35,
    "twilight-in-the-wilderness",
    "Twilight in the Wilderness",
    "Frederic Edwin Church",
    "1860",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    399,
    "dark",
  ),
  makeArtEntry(
    36,
    "rocky-wooded-landscape",
    "Rocky, Wooded Landscape with a Dell and Weir",
    "Thomas Gainsborough",
    "c. 1782-1783",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    299,
    "light",
  ),
  makeArtEntry(
    37,
    "landscape-with-castle-and-shepherds",
    "Landscape with Castle and Shepherds",
    "Achille Etna Michallon",
    "1814",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    299,
    "light",
  ),
  makeArtEntry(
    38,
    "landscape-with-a-windmill",
    "Landscape with a Windmill",
    "Jacob van Ruisdael",
    "1646",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    299,
    "dark",
  ),
  makeArtEntry(
    39,
    "water-lily-pond",
    "Water Lily Pond",
    "Claude Monet",
    "1917-19",
    "Art Institute of Chicago",
    "CC0 1.0 Public Domain",
    399,
    "dark",
  ),
  makeArtEntry(
    40,
    "branch-hill-pond",
    "Branch Hill Pond, Hampstead",
    "John Constable",
    "1828",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    299,
    "dark",
  ),
  makeArtEntry(
    41,
    "under-the-wave-off-kanagawa",
    "Under the Wave off Kanagawa (The Great Wave)",
    "Katsushika Hokusai",
    "1830/33",
    "Art Institute of Chicago",
    "CC0 1.0 Public Domain",
    399,
    "light",
  ),
  makeArtEntry(
    42,
    "marine",
    "Marine",
    "William Merritt Chase",
    "c. 1888",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    299,
    "dark",
  ),
  makeArtEntry(
    43,
    "boats-at-berck-sur-mer",
    "Boats at Berck-sur-Mer",
    "Édouard Manet",
    "1873",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    299,
    "dark",
  ),
  makeArtEntry(
    44,
    "mount-vesuvius-at-midnight",
    "Mount Vesuvius at Midnight",
    "Albert Bierstadt",
    "1868",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    399,
    "dark",
  ),
  makeArtEntry(
    45,
    "burning-of-houses-of-parliament",
    "Burning of the Houses of Lords and Commons",
    "Joseph Mallord William Turner",
    "1835",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    399,
    "gold",
  ),
  makeArtEntry(
    46,
    "rocky-mountains-landers-peak",
    "The Rocky Mountains, Lander's Peak",
    "Albert Bierstadt",
    "1863",
    "The Metropolitan Museum of Art",
    "CC0 1.0 Public Domain",
    399,
    "dark",
  ),
  makeArtEntry(
    47,
    "northeaster",
    "Northeaster",
    "Winslow Homer",
    "1895",
    "The Metropolitan Museum of Art",
    "CC0 1.0 Public Domain",
    399,
    "dark",
  ),
  makeArtEntry(
    48,
    "tiger-in-bamboo",
    "Tiger in Bamboo",
    "Sesson Shūkei",
    "c. 1546-56",
    "Cleveland Museum of Art",
    "CC0 1.0 Public Domain",
    299,
    "light",
  ),
  makeArtEntry(
    49,
    "barks-fleeing-before-the-storm",
    "Barks Fleeing Before the Storm",
    "Jules Dupré",
    "1870-75",
    "Art Institute of Chicago",
    "CC0 1.0 Public Domain",
    299,
    "dark",
  ),
  makeArtEntry(
    50,
    "cliff-walk-at-pourville",
    "Cliff Walk at Pourville",
    "Claude Monet",
    "1882",
    "Art Institute of Chicago",
    "CC0 1.0 Public Domain",
    299,
    "light",
  ),
  makeArtEntry(
    51,
    "the-poets-garden",
    "The Poet's Garden",
    "Vincent van Gogh",
    "1888",
    "Art Institute of Chicago",
    "CC0 1.0 Public Domain",
    299,
    "dark",
  ),
  makeArtEntry(
    52,
    "poppy-field-giverny",
    "Poppy Field (Giverny)",
    "Claude Monet",
    "1890-91",
    "Art Institute of Chicago",
    "CC0 1.0 Public Domain",
    299,
    "dark",
  ),
  makeArtEntry(
    53,
    "the-petite-creuse-river",
    "The Petite Creuse River",
    "Claude Monet",
    "1889",
    "Art Institute of Chicago",
    "CC0 1.0 Public Domain",
    299,
    "light",
  ),
];

/** Complete ordered roster of all 53 approved playmats (none free by default). */
export const ALL_PLAYMATS: readonly PlaymatSpec[] = [
  ...ART_PLAYMATS,
  ...BRAND_PLAYMATS,
  ...MATERIAL_PLAYMATS,
];

const PLAYMAT_LOOKUP = new Map<string, PlaymatSpec>();
for (const mat of ALL_PLAYMATS) {
  PLAYMAT_LOOKUP.set(mat.slug, mat);
  PLAYMAT_LOOKUP.set(mat.canonicalId, mat);
  PLAYMAT_LOOKUP.set(mat.previewId, mat);
  PLAYMAT_LOOKUP.set(mat.assetRef, mat);
}

/** Resolve any canonical UUID, preview ID, asset ref, or slug to a known playmat spec. */
export function resolvePlaymat(
  idOrRef: string | null | undefined,
): PlaymatSpec | null {
  if (!idOrRef) return null;
  return PLAYMAT_LOOKUP.get(idOrRef) ?? null;
}

/** Relative runtime asset paths for the 21 locked raster art playmats. */
export const PLAYMAT_ART_ASSET_PATHS: readonly string[] = ART_PLAYMATS.map(
  (mat) => mat.artAssetPath!,
);

// ---------------------------------------------------------------------------
// Deterministic PRNG & Noise Helpers
// ---------------------------------------------------------------------------
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function hashStr(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h | 0;
}

function makeNoise(seed: number) {
  const r = mulberry32(seed);
  const p = new Uint8Array(512);
  const v = new Float32Array(256);
  for (let i = 0; i < 256; i++) {
    p[i] = i;
    v[i] = r();
  }
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(r() * (i + 1));
    const tmp = p[i] ?? 0;
    p[i] = p[j] ?? 0;
    p[j] = tmp;
  }
  for (let i = 0; i < 256; i++) p[i + 256] = p[i] ?? 0;
  const fade = (t: number) => t * t * t * (t * (t * 6 - 15) + 10);
  const sample = (x: number, y: number) => {
    const xi = Math.floor(x) & 255;
    const yi = Math.floor(y) & 255;
    const xf = x - Math.floor(x);
    const yf = y - Math.floor(y);
    const u = fade(xf);
    const w = fade(yf);
    const aa = v[p[((p[xi] ?? 0) + yi) & 511] ?? 0] ?? 0;
    const ba = v[p[((p[(xi + 1) & 255] ?? 0) + yi) & 511] ?? 0] ?? 0;
    const ab = v[p[((p[xi] ?? 0) + yi + 1) & 511] ?? 0] ?? 0;
    const bb = v[p[((p[(xi + 1) & 255] ?? 0) + yi + 1) & 511] ?? 0] ?? 0;
    return aa + (ba - aa) * u + (ab + (bb - ab) * u - (aa + (ba - aa) * u)) * w;
  };
  const fbm = (x: number, y: number, oct = 4) => {
    let s = 0;
    let a = 0.5;
    let f = 1;
    let norm = 0;
    for (let o = 0; o < oct; o++) {
      s += sample(x * f, y * f) * a;
      norm += a;
      a *= 0.5;
      f *= 2.02;
    }
    return s / norm;
  };
  return { sample, fbm };
}

// ---------------------------------------------------------------------------
// Procedural Edge Renderer: Braided Merrowed Stitching & Leather Saddle Frame
// ---------------------------------------------------------------------------
type PerimSample = {
  x: number;
  y: number;
  nx: number;
  ny: number;
  tx: number;
  ty: number;
};

function rgb(
  c: readonly [number, number, number],
  delta = 0,
  a = 1,
): string {
  const r = Math.max(0, Math.min(255, Math.round(c[0] + delta)));
  const g = Math.max(0, Math.min(255, Math.round(c[1] + delta)));
  const b = Math.max(0, Math.min(255, Math.round(c[2] + delta)));
  return a < 1 ? `rgba(${r},${g},${b},${a})` : `rgb(${r},${g},${b})`;
}

function sampleRoundedRectPerimeter(
  w: number,
  h: number,
  r: number,
  step: number,
): PerimSample[] {
  const pts: PerimSample[] = [];
  const pushLine = (
    x0: number,
    y0: number,
    x1: number,
    y1: number,
    nx: number,
    ny: number,
  ) => {
    const len = Math.hypot(x1 - x0, y1 - y0);
    const n = Math.max(1, Math.round(len / step));
    const tx = (x1 - x0) / len;
    const ty = (y1 - y0) / len;
    for (let i = 0; i < n; i++) {
      const t = i / n;
      pts.push({
        x: x0 + (x1 - x0) * t,
        y: y0 + (y1 - y0) * t,
        nx,
        ny,
        tx,
        ty,
      });
    }
  };
  const pushArc = (cx: number, cy: number, a0: number, a1: number) => {
    const arcLen = Math.abs(a1 - a0) * r;
    const n = Math.max(2, Math.round(arcLen / step));
    for (let i = 0; i < n; i++) {
      const a = a0 + ((a1 - a0) * i) / n;
      const nx = Math.cos(a);
      const ny = Math.sin(a);
      pts.push({ x: cx + nx * r, y: cy + ny * r, nx, ny, tx: -ny, ty: nx });
    }
  };

  pushLine(r, 0, w - r, 0, 0, -1);
  pushArc(w - r, r, -Math.PI / 2, 0);
  pushLine(w, r, w, h - r, 1, 0);
  pushArc(w - r, h - r, 0, Math.PI / 2);
  pushLine(w - r, h, r, h, 0, 1);
  pushArc(r, h - r, Math.PI / 2, Math.PI);
  pushLine(0, h - r, 0, r, -1, 0);
  pushArc(r, r, Math.PI, Math.PI * 1.5);
  return pts;
}

function roundedRectPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + w - r, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + r);
  ctx.lineTo(x + w, y + h - r);
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  ctx.lineTo(x + r, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}

/** Draw the playmat's merrowed overlock stitching or debossed leather saddle-stitch frame. */
export function drawPlaymatEdge(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  edge: PlaymatEdgeStyle,
): void {
  if (edge.kind === "none") return;
  const scale = w / 960;
  const cornerR = 19 * scale;

  if (edge.kind === "saddle") {
    const creaseInset = 11 * scale;
    const stitchInset = 18 * scale;

    ctx.save();
    roundedRectPath(
      ctx,
      1.5 * scale,
      1.5 * scale,
      w - 3 * scale,
      h - 3 * scale,
      Math.max(3, cornerR - 1.5 * scale),
    );
    ctx.lineWidth = Math.max(1, 3 * scale);
    ctx.strokeStyle = "rgba(0, 0, 0, 0.45)";
    ctx.stroke();
    ctx.restore();

    ctx.save();
    roundedRectPath(
      ctx,
      creaseInset,
      creaseInset,
      w - creaseInset * 2,
      h - creaseInset * 2,
      Math.max(3, cornerR - creaseInset * 0.65),
    );
    ctx.lineWidth = Math.max(0.8, 2.4 * scale);
    ctx.strokeStyle = edge.groove;
    ctx.stroke();
    ctx.restore();

    ctx.save();
    roundedRectPath(
      ctx,
      creaseInset + 1.4 * scale,
      creaseInset + 1.4 * scale,
      w - (creaseInset + 1.4 * scale) * 2,
      h - (creaseInset + 1.4 * scale) * 2,
      Math.max(3, cornerR - creaseInset * 0.65),
    );
    ctx.lineWidth = Math.max(0.5, 0.9 * scale);
    ctx.strokeStyle = "rgba(255, 255, 255, 0.14)";
    ctx.stroke();
    ctx.restore();

    if (edge.trim) {
      const trimInset = 25 * scale;
      ctx.save();
      roundedRectPath(
        ctx,
        trimInset,
        trimInset,
        w - trimInset * 2,
        h - trimInset * 2,
        Math.max(2, cornerR - trimInset * 0.5),
      );
      ctx.lineWidth = Math.max(0.6, 1.25 * scale);
      ctx.strokeStyle = edge.trim;
      ctx.stroke();
      ctx.restore();
    }

    const sw = w - stitchInset * 2;
    const sh = h - stitchInset * 2;
    const sr = Math.max(3, cornerR - stitchInset * 0.6);
    const step = Math.max(4, 12 * scale);
    const pts = sampleRoundedRectPerimeter(sw, sh, sr, step);

    for (const p of pts) {
      const cx = p.x + stitchInset;
      const cy = p.y + stitchInset;
      const half = Math.max(1.2, 3.6 * scale);
      const dx = p.tx * half + p.nx * (0.45 * scale);
      const dy = p.ty * half + p.ny * (0.45 * scale);

      ctx.beginPath();
      ctx.moveTo(cx - dx, cy - dy + 0.8 * scale);
      ctx.lineTo(cx + dx, cy + dy + 0.8 * scale);
      ctx.lineWidth = Math.max(1, 2.6 * scale);
      ctx.lineCap = "round";
      ctx.strokeStyle = "rgba(0, 0, 0, 0.65)";
      ctx.stroke();

      ctx.beginPath();
      ctx.moveTo(cx - dx, cy - dy);
      ctx.lineTo(cx + dx, cy + dy);
      ctx.lineWidth = Math.max(0.8, 1.7 * scale);
      ctx.lineCap = "round";
      ctx.strokeStyle = edge.thread;
      ctx.stroke();
    }
    return;
  }

  // Braided merrowed overlock border
  const pal = edge.palette;
  const bandW = Math.max(3, 9.5 * scale);
  const step = Math.max(2.1, 4.6 * scale);
  const rng = mulberry32(hashStr(edge.key));

  ctx.save();
  roundedRectPath(
    ctx,
    bandW * 0.6,
    bandW * 0.6,
    w - bandW * 1.2,
    h - bandW * 1.2,
    Math.max(3, cornerR - bandW * 0.5),
  );
  ctx.lineWidth = 7.5 * scale;
  ctx.strokeStyle = "rgba(0, 0, 0, 0.42)";
  if ("filter" in ctx) ctx.filter = `blur(${(2.4 * scale).toFixed(1)}px)`;
  ctx.stroke();
  ctx.restore();

  ctx.save();
  roundedRectPath(
    ctx,
    bandW * 0.45,
    bandW * 0.45,
    w - bandW * 0.9,
    h - bandW * 0.9,
    Math.max(3, cornerR - bandW * 0.4),
  );
  ctx.lineWidth = bandW * 0.98;
  ctx.strokeStyle = pal.crevice;
  ctx.stroke();
  ctx.restore();

  const samples = sampleRoundedRectPerimeter(w, h, cornerR, step);
  const slant = 2.6 * scale;
  const coilW = Math.max(1.3, 3.2 * scale);
  const isDark = pal.base[0] + pal.base[1] + pal.base[2] < 160;

  for (const p of samples) {
    const jitter = (rng() - 0.5) * 14;
    const lenJitter = (rng() - 0.5) * 0.8 * scale;

    const xOut = p.x - p.nx * (0.4 * scale) - p.tx * slant * 0.45;
    const yOut = p.y - p.ny * (0.4 * scale) - p.ty * slant * 0.45;
    const xIn = p.x - p.nx * (bandW + lenJitter) + p.tx * slant * 0.55;
    const yIn = p.y - p.ny * (bandW + lenJitter) + p.ty * slant * 0.55;

    const lightDot = -(p.nx * 0.65 + p.ny * 0.76);
    const shadeBias = lightDot * 14 + jitter;

    ctx.beginPath();
    ctx.moveTo(xOut + p.tx * (0.9 * scale), yOut + p.ty * (0.9 * scale));
    ctx.lineTo(xIn + p.tx * (0.9 * scale), yIn + p.ty * (0.9 * scale));
    ctx.lineWidth = coilW * 1.05;
    ctx.lineCap = "round";
    ctx.strokeStyle = pal.crevice;
    ctx.stroke();

    const grad = ctx.createLinearGradient(xOut, yOut, xIn, yIn);
    grad.addColorStop(0, rgb(pal.lo, shadeBias * 0.6));
    grad.addColorStop(0.32, rgb(pal.hi, shadeBias));
    grad.addColorStop(0.58, rgb(pal.base, shadeBias));
    grad.addColorStop(1, rgb(pal.lo, shadeBias * 0.5));

    ctx.beginPath();
    ctx.moveTo(xOut, yOut);
    ctx.lineTo(xIn, yIn);
    ctx.lineWidth = coilW * 0.84;
    ctx.lineCap = "round";
    ctx.strokeStyle = grad;
    ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(
      xOut + (xIn - xOut) * 0.16 - p.tx * (0.45 * scale),
      yOut + (yIn - yOut) * 0.16 - p.ty * (0.45 * scale),
    );
    ctx.lineTo(
      xOut + (xIn - xOut) * 0.72 - p.tx * (0.45 * scale),
      yOut + (yIn - yOut) * 0.72 - p.ty * (0.45 * scale),
    );
    ctx.lineWidth = Math.max(0.55, coilW * 0.28);
    ctx.lineCap = "round";
    ctx.strokeStyle = rgb(pal.hi, shadeBias * 0.8, isDark ? 0.55 : 0.72);
    ctx.stroke();
  }

  ctx.save();
  const inset = bandW + 0.8 * scale;
  roundedRectPath(
    ctx,
    inset,
    inset,
    w - inset * 2,
    h - inset * 2,
    Math.max(2, cornerR - inset),
  );
  ctx.setLineDash([Math.max(1.2, 2.8 * scale), Math.max(1.0, 2.2 * scale)]);
  ctx.lineWidth = Math.max(0.6, 1.15 * scale);
  ctx.strokeStyle = pal.lockCord;
  ctx.stroke();
  ctx.restore();
}

// ---------------------------------------------------------------------------
// Brand SVG Generator (Official Spellwake Flame Paths)
// ---------------------------------------------------------------------------
function officialSpellwakeLogoSvg(
  uid: string,
  cx: number,
  cy: number,
  scale: number,
  opacity: number,
  mode: "gold" | "silver" | "tinted",
  tintColor = "#d9b772",
): string {
  const tx = (cx - 30 * scale).toFixed(1);
  const ty = (cy - 35 * scale).toFixed(1);
  if (mode === "gold") {
    return `<defs>
      <linearGradient id="${uid}-ur" x1="23" y1="7" x2="47" y2="53" gradientUnits="userSpaceOnUse">
        <stop stop-color="#ffd686"/><stop offset=".4" stop-color="#ffbd5f"/>
        <stop offset=".75" stop-color="#ef9c3c"/><stop offset="1" stop-color="#dc973b"/>
      </linearGradient>
      <linearGradient id="${uid}-uh" x1="24" y1="8" x2="34" y2="35" gradientUnits="userSpaceOnUse">
        <stop stop-color="#fff1bc" stop-opacity=".75"/><stop offset="1" stop-color="#ffe8a2" stop-opacity=".2"/>
      </linearGradient>
      <linearGradient id="${uid}-ll" x1="10" y1="19" x2="29" y2="59" gradientUnits="userSpaceOnUse">
        <stop stop-color="#ffd98e"/><stop offset=".34" stop-color="#f5b256"/>
        <stop offset=".66" stop-color="#ef9c3e"/><stop offset="1" stop-color="#d8822a"/>
      </linearGradient>
    </defs>
    <g transform="translate(${tx},${ty}) scale(${scale})" opacity="${opacity}">
      <path fill="url(#${uid}-ur)" fill-rule="evenodd" d="M35.7 4.3 C32.5 9.3 30.8 12.5 31.6 17 C31.7 22.1 37.8 25.3 44 28.7 C50.4 31.9 52.8 34.3 51.8 40.1 C51.4 46.4 46.2 51.5 37.5 56.7 C41.2 51.9 43.6 46.7 41.3 41.7 C39.7 38.3 35.8 36.3 29.8 33.2 C23.9 29.7 20.2 26.2 20.3 20.8 C20.2 14.5 25.5 10 35.7 4.3 Z"/>
      <path fill="url(#${uid}-uh)" d="M35.7 4.3 C25.5 10 20.2 14.5 20.3 20.8 C20.2 26.2 23.9 29.7 29.8 33.2 L33.7 35.1 C26.1 29.6 21.7 26.4 21.5 21.1 C21.2 15.2 26.9 10.2 35.7 4.3 Z"/>
      <path fill="#ffd47f" fill-opacity=".5" d="M44 28.7 C50.4 31.9 52.8 34.3 51.8 40.1 C51.4 46.4 46.2 51.5 37.5 56.7 C45.6 50.7 50.2 45.9 50.7 40.1 C51.1 34.7 48.7 32.1 44 28.7 Z"/>
      <path fill="url(#${uid}-ll)" fill-rule="evenodd" d="M18.1 16.9 C12.9 21.3 5.8 28.2 6.1 34 C6.1 40.6 13.4 43.4 20.5 46.8 C27.1 50.3 29.4 53.5 22.2 65.1 C29.7 61 36.7 55.8 36.2 48.4 C35.8 42.3 30 39.9 23.4 36 C14.7 30.9 12.4 26.8 18.1 16.9 Z"/>
      <path fill="#ffe9ab" opacity=".55" d="M18.1 16.9 C12.7 23.2 8.5 28.4 7.7 33.5 C6.7 37.8 8.8 40.5 12.6 42.7 C7.4 40.2 5.6 37.4 6.1 34 C5.8 28.2 12.9 21.3 18.1 16.9 Z"/>
    </g>`;
  }
  if (mode === "silver") {
    return `<defs>
      <linearGradient id="${uid}-sr" x1="20" y1="6" x2="48" y2="56" gradientUnits="userSpaceOnUse">
        <stop stop-color="#f2f6ff"/><stop offset=".42" stop-color="#cfd9ee"/>
        <stop offset=".78" stop-color="#9fb0d4"/><stop offset="1" stop-color="#7c8eb5"/>
      </linearGradient>
      <linearGradient id="${uid}-sl" x1="8" y1="18" x2="32" y2="62" gradientUnits="userSpaceOnUse">
        <stop stop-color="#e8effc"/><stop offset=".48" stop-color="#bcc8e2"/>
        <stop offset="1" stop-color="#8799bf"/>
      </linearGradient>
    </defs>
    <g transform="translate(${tx},${ty}) scale(${scale})" opacity="${opacity}">
      <path fill="url(#${uid}-sr)" fill-rule="evenodd" d="M35.7 4.3 C32.5 9.3 30.8 12.5 31.6 17 C31.7 22.1 37.8 25.3 44 28.7 C50.4 31.9 52.8 34.3 51.8 40.1 C51.4 46.4 46.2 51.5 37.5 56.7 C41.2 51.9 43.6 46.7 41.3 41.7 C39.7 38.3 35.8 36.3 29.8 33.2 C23.9 29.7 20.2 26.2 20.3 20.8 C20.2 14.5 25.5 10 35.7 4.3 Z"/>
      <path fill="#ffffff" fill-opacity=".42" d="M35.7 4.3 C25.5 10 20.2 14.5 20.3 20.8 C20.2 26.2 23.9 29.7 29.8 33.2 L33.7 35.1 C26.1 29.6 21.7 26.4 21.5 21.1 C21.2 15.2 26.9 10.2 35.7 4.3 Z"/>
      <path fill="url(#${uid}-sl)" fill-rule="evenodd" d="M18.1 16.9 C12.9 21.3 5.8 28.2 6.1 34 C6.1 40.6 13.4 43.4 20.5 46.8 C27.1 50.3 29.4 53.5 22.2 65.1 C29.7 61 36.7 55.8 36.2 48.4 C35.8 42.3 30 39.9 23.4 36 C14.7 30.9 12.4 26.8 18.1 16.9 Z"/>
      <path fill="#ffffff" opacity=".4" d="M18.1 16.9 C12.7 23.2 8.5 28.4 7.7 33.5 C6.7 37.8 8.8 40.5 12.6 42.7 C7.4 40.2 5.6 37.4 6.1 34 C5.8 28.2 12.9 21.3 18.1 16.9 Z"/>
    </g>`;
  }
  return `<g transform="translate(${tx},${ty}) scale(${scale})" opacity="${opacity}">
    <path fill="${tintColor}" fill-rule="evenodd" d="M35.7 4.3 C32.5 9.3 30.8 12.5 31.6 17 C31.7 22.1 37.8 25.3 44 28.7 C50.4 31.9 52.8 34.3 51.8 40.1 C51.4 46.4 46.2 51.5 37.5 56.7 C41.2 51.9 43.6 46.7 41.3 41.7 C39.7 38.3 35.8 36.3 29.8 33.2 C23.9 29.7 20.2 26.2 20.3 20.8 C20.2 14.5 25.5 10 35.7 4.3 Z"/>
    <path fill="${tintColor}" opacity="0.88" fill-rule="evenodd" d="M18.1 16.9 C12.9 21.3 5.8 28.2 6.1 34 C6.1 40.6 13.4 43.4 20.5 46.8 C27.1 50.3 29.4 53.5 22.2 65.1 C29.7 61 36.7 55.8 36.2 48.4 C35.8 42.3 30 39.9 23.4 36 C14.7 30.9 12.4 26.8 18.1 16.9 Z"/>
  </g>`;
}

function makeBrandSvg(opts: {
  bgA: string;
  bgB: string;
  accent: string;
  decor: string;
  showLogo?: boolean;
  logoMode?: "gold" | "silver" | "tinted";
  logoTint?: string;
  logoOpacity?: number;
  logoScale?: number;
  borderDouble?: boolean;
  borderOpacity?: number;
  cornerTicks?: boolean;
  hasStitch?: boolean;
}): string {
  const bOp = opts.borderOpacity ?? 0.52;
  const pad = opts.hasStitch ? 25 : 16;
  const uid =
    "g" + Math.abs(hashStr(opts.bgA + opts.bgB + opts.decor)).toString(36);
  const logoHtml =
    opts.showLogo !== false
      ? officialSpellwakeLogoSvg(
          uid + "l",
          320,
          198,
          opts.logoScale ?? 2.0,
          opts.logoOpacity ?? 0.55,
          opts.logoMode ?? "gold",
          opts.logoTint ?? opts.accent,
        )
      : "";
  const w1 = 640 - pad * 2;
  const h1 = 400 - pad * 2;
  const p2 = pad + 7;
  const w2 = 640 - p2 * 2;
  const h2 = 400 - p2 * 2;
  const tLen = 26;
  const dotOff = pad + 13;

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 400" width="640" height="400">
  <defs>
    <radialGradient id="${uid}" cx="50%" cy="46%" r="68%">
      <stop offset="0%" stop-color="${opts.bgA}"/>
      <stop offset="100%" stop-color="${opts.bgB}"/>
    </radialGradient>
  </defs>
  <rect width="640" height="400" fill="url(#${uid})"/>
  ${opts.decor}
  ${
    bOp > 0
      ? opts.borderDouble !== false
        ? `<rect x="${pad}" y="${pad}" width="${w1}" height="${h1}" rx="7" fill="none" stroke="${opts.accent}" stroke-width="1.25" opacity="${bOp}"/>
           <rect x="${p2}" y="${p2}" width="${w2}" height="${h2}" rx="4.5" fill="none" stroke="${opts.accent}" stroke-width="0.65" opacity="${(bOp * 0.54).toFixed(2)}"/>`
        : `<rect x="${pad}" y="${pad}" width="${w1}" height="${h1}" rx="6" fill="none" stroke="${opts.accent}" stroke-width="1.05" opacity="${bOp}"/>`
      : ""
  }
  ${
    opts.cornerTicks
      ? `<g stroke="${opts.accent}" stroke-width="1.3" fill="none" opacity="${Math.min(0.65, bOp + 0.08).toFixed(2)}">
          <path d="M${pad} ${pad + tLen} L${pad} ${pad} L${pad + tLen} ${pad}"/>
          <path d="M${640 - pad - tLen} ${pad} L${640 - pad} ${pad} L${640 - pad} ${pad + tLen}"/>
          <path d="M${pad} ${400 - pad - tLen} L${pad} ${400 - pad} L${pad + tLen} ${400 - pad}"/>
          <path d="M${640 - pad - tLen} ${400 - pad} L${640 - pad} ${400 - pad} L${640 - pad} ${400 - pad - tLen}"/>
          <circle cx="${dotOff}" cy="${dotOff}" r="2.1" fill="${opts.accent}"/>
          <circle cx="${640 - dotOff}" cy="${dotOff}" r="2.1" fill="${opts.accent}"/>
          <circle cx="${dotOff}" cy="${400 - dotOff}" r="2.1" fill="${opts.accent}"/>
          <circle cx="${640 - dotOff}" cy="${400 - dotOff}" r="2.1" fill="${opts.accent}"/>
        </g>`
      : ""
  }
  ${logoHtml}
</svg>`;
}

const starPoints = (cx: number, cy: number, r: number, n: number) =>
  Array.from({ length: n }, (_, i) => {
    const a = -Math.PI / 2 + (i * Math.PI * 2) / n;
    return [cx + Math.cos(a) * r, cy + Math.sin(a) * r] as const;
  });

const BRAND_SVG_CACHE = new Map<string, string>();

/** Build the deterministic SVG markup for any Brand & Ornamental playmat slug. */
export function buildBrandPlaymatSvg(slug: string): string | null {
  const cached = BRAND_SVG_CACHE.get(slug);
  if (cached) return cached;

  let svg: string | null = null;
  switch (slug) {
    case "obsidian-and-brass":
      svg = makeBrandSvg({
        bgA: "#22242a",
        bgB: "#0d0e11",
        accent: "#c9a45c",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "gold",
        logoOpacity: 0.78,
        logoScale: 2.0,
        decor: `<g stroke="#c9a45c" fill="none" opacity="0.2">
          <path d="M32 88 L88 32 M552 32 L608 88 M32 312 L88 368 M552 368 L608 312" stroke-width="1"/>
          <rect x="48" y="48" width="544" height="304" rx="4" stroke-width="0.7" stroke-dasharray="2 4"/>
        </g>`,
      });
      break;
    case "slate-filigree":
      svg = makeBrandSvg({
        bgA: "#2c3440",
        bgB: "#171c23",
        accent: "#a9b9cf",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "tinted",
        logoOpacity: 0.42,
        logoScale: 1.95,
        decor: `<g stroke="#a9b9cf" fill="none" opacity="0.22">
          <rect x="40" y="40" width="560" height="320" rx="6" stroke-width="0.9"/>
          <path d="M126 40 L126 60 M514 40 L514 60 M126 340 L126 360 M514 340 L514 360" stroke-width="1"/>
          <circle cx="320" cy="198" r="108" stroke-width="0.8" stroke-dasharray="6 4"/>
        </g>`,
      });
      break;
    case "astral-compass":
      svg = makeBrandSvg({
        bgA: "#1a2a47",
        bgB: "#0c1424",
        accent: "#d4b36f",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "gold",
        logoOpacity: 0.72,
        logoScale: 1.9,
        decor: `<g stroke="#d4b36f" fill="none" opacity="0.26">
          <circle cx="320" cy="198" r="140" stroke-width="1"/>
          <circle cx="320" cy="198" r="130" stroke-width="0.6" stroke-dasharray="2 6"/>
          <line x1="66" y1="198" x2="574" y2="198" stroke-width="0.7" opacity="0.45"/>
          <line x1="320" y1="42" x2="320" y2="354" stroke-width="0.7" opacity="0.45"/>
        </g>`,
      });
      break;
    case "archivists-vellum":
      svg = makeBrandSvg({
        bgA: "#dfd4bc",
        bgB: "#bfb090",
        accent: "#594832",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "tinted",
        logoOpacity: 0.34,
        logoScale: 1.95,
        decor: `<g stroke="#594832" fill="none" opacity="0.28">
          <rect x="40" y="40" width="560" height="320" rx="5" stroke-width="0.9"/>
          <circle cx="320" cy="198" r="114" stroke-width="0.9"/>
          <circle cx="320" cy="198" r="124" stroke-width="0.6" stroke-dasharray="3 4"/>
        </g>`,
      });
      break;
    case "arcane-orrery":
      svg = makeBrandSvg({
        bgA: "#1c2240",
        bgB: "#0b0e1e",
        accent: "#d6b36c",
        cornerTicks: true,
        hasStitch: false,
        logoMode: "gold",
        logoOpacity: 0.72,
        logoScale: 1.85,
        decor: `<g stroke="#d6b36c" fill="none" opacity="0.34">
          <circle cx="320" cy="198" r="158" stroke-width="1"/>
          <circle cx="320" cy="198" r="132" stroke-width="0.7" stroke-dasharray="4 4"/>
          <circle cx="320" cy="198" r="94" stroke-width="1.2"/>
          <ellipse cx="320" cy="198" rx="220" ry="74" stroke-width="0.9" transform="rotate(-18 320 198)"/>
          <ellipse cx="320" cy="198" rx="220" ry="74" stroke-width="0.9" transform="rotate(18 320 198)"/>
          <circle cx="454" cy="152" r="6" fill="#d6b36c"/><circle cx="186" cy="244" r="5" fill="#d6b36c"/>
          <circle cx="390" cy="110" r="4" fill="#d6b36c"/>
        </g>`,
      });
      break;
    case "fivefold-mana-star": {
      const pent = starPoints(320, 198, 122, 5);
      const pentLines = pent
        .map((p, i) => {
          const q = pent[(i + 2) % 5]!;
          return `<line x1="${p[0].toFixed(1)}" y1="${p[1].toFixed(1)}" x2="${q[0].toFixed(1)}" y2="${q[1].toFixed(1)}"/>`;
        })
        .join("");
      const manaOrder = [
        { rim: "#f5ebd2", core: "#fff7e0" },
        { rim: "#58aceb", core: "#79c2f7" },
        { rim: "#8d78a6", core: "#352c42" },
        { rim: "#e66447", core: "#f07c60" },
        { rim: "#56ab6a", core: "#72c484" },
      ];
      const pentNodes = pent
        .map(
          (p, i) =>
            `<circle cx="${p[0].toFixed(1)}" cy="${p[1].toFixed(1)}" r="10.5" fill="#14171d" stroke="${manaOrder[i]!.rim}" stroke-width="2.1"/>
             <circle cx="${p[0].toFixed(1)}" cy="${p[1].toFixed(1)}" r="5" fill="${manaOrder[i]!.core}" stroke="${manaOrder[i]!.rim}" stroke-width="${i === 2 ? "1.1" : "0"}"/>`,
        )
        .join("");
      svg = makeBrandSvg({
        bgA: "#23262e",
        bgB: "#111318",
        accent: "#c9b892",
        hasStitch: true,
        logoMode: "tinted",
        logoOpacity: 0.42,
        logoScale: 1.15,
        decor: `<g stroke="#c9b892" stroke-width="1" fill="none" opacity="0.34">
          <circle cx="320" cy="198" r="122"/>
          <circle cx="320" cy="198" r="136" stroke-dasharray="2 5"/>
          <circle cx="320" cy="198" r="46" opacity="0.45"/>
          ${pentLines}
        </g>${pentNodes}`,
      });
      break;
    }
    case "gilded-sunburst": {
      const rays = Array.from({ length: 24 }, (_, i) => {
        const a = (i * Math.PI) / 12;
        return `<line x1="320" y1="198" x2="${(320 + Math.cos(a) * 340).toFixed(1)}" y2="${(198 + Math.sin(a) * 340).toFixed(1)}"/>`;
      }).join("");
      svg = makeBrandSvg({
        bgA: "#1b362b",
        bgB: "#0b1914",
        accent: "#d6b365",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "gold",
        logoOpacity: 0.84,
        logoScale: 1.88,
        decor: `<g stroke="#d6b365" stroke-width="0.9" opacity="0.18">${rays}</g>
                <circle cx="320" cy="198" r="80" fill="#10221b" stroke="#d6b365" stroke-width="1.4" opacity="0.82"/>`,
      });
      break;
    }
    case "crimson-sanctum":
      svg = makeBrandSvg({
        bgA: "#4a171c",
        bgB: "#1f080b",
        accent: "#c49a5c",
        borderDouble: false,
        borderOpacity: 0.24,
        cornerTicks: false,
        hasStitch: true,
        logoMode: "gold",
        logoOpacity: 0.74,
        logoScale: 1.92,
        decor: `<g stroke="#d6ae66" fill="none" stroke-width="1.0" opacity="0.24">
          <path d="M126 372 Q126 122 320 48 Q514 122 514 372"/>
          <path d="M156 372 Q156 150 320 82 Q484 150 484 372" stroke-dasharray="3 4"/>
          <circle cx="320" cy="112" r="21"/>
        </g>`,
      });
      break;
    case "verdant-leylines": {
      const topoWaves = Array.from({ length: 11 }, (_, i) => {
        const y = 34 + i * 33;
        return `<path d="M 0 ${y} Q 160 ${y - 26 + (i % 3) * 14} 320 ${y + 8} T 640 ${y - 6}" fill="none" stroke="#85b58a" stroke-width="1" opacity="${0.14 + (i % 3) * 0.06}"/>`;
      }).join("");
      svg = makeBrandSvg({
        bgA: "#1f3829",
        bgB: "#0f1e16",
        accent: "#a4c79d",
        hasStitch: true,
        logoMode: "tinted",
        logoOpacity: 0.42,
        logoScale: 1.95,
        decor: topoWaves,
      });
      break;
    }
    case "celestial-meridian": {
      const stars: string[] = [];
      for (let i = 0; i < 65; i++) {
        const x = ((i * 97 + 41) % 590) + 25;
        const y = ((i * 53 + 29) % 350) + 25;
        const r = i % 7 === 0 ? 2.1 : i % 3 === 0 ? 1.3 : 0.8;
        stars.push(
          `<circle cx="${x}" cy="${y}" r="${r}" fill="#e9dcb8" opacity="${0.25 + (i % 5) * 0.14}"/>`,
        );
      }
      svg = makeBrandSvg({
        bgA: "#142038",
        bgB: "#080d1a",
        accent: "#cfd9eb",
        hasStitch: false,
        logoMode: "silver",
        logoOpacity: 0.56,
        logoScale: 1.9,
        decor: `<g stroke="#9ab0d9" fill="none" stroke-width="0.7" opacity="0.22">
          <line x1="20" y1="198" x2="620" y2="198"/><line x1="320" y1="20" x2="320" y2="380"/>
          <path d="M20 290 Q320 70 620 290" stroke-dasharray="5 4" opacity="0.8"/>
          <circle cx="320" cy="198" r="145"/>
        </g>${stars.join("")}`,
      });
      break;
    }
    case "sunken-foundry":
      svg = makeBrandSvg({
        bgA: "#2a231e",
        bgB: "#14100e",
        accent: "#cc935c",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "tinted",
        logoTint: "#bd8958",
        logoOpacity: 0.34,
        logoScale: 1.82,
        decor: `<g stroke="#cc935c" fill="none" stroke-width="0.6" opacity="0.16">
          ${Array.from({ length: 15 }, (_, i) => `<line x1="${40 + i * 40}" y1="25" x2="${40 + i * 40}" y2="375"/>`).join("")}
          ${Array.from({ length: 9 }, (_, i) => `<line x1="25" y1="${40 + i * 40}" x2="615" y2="${40 + i * 40}"/>`).join("")}
        </g>
        <g stroke="#cc935c" fill="none" opacity="0.28">
          <circle cx="154" cy="198" r="66" stroke-width="1.3" stroke-dasharray="8 6"/>
          <circle cx="486" cy="198" r="66" stroke-width="1.3" stroke-dasharray="8 6"/>
        </g>`,
      });
      break;
    case "alchemists-circle":
      svg = makeBrandSvg({
        bgA: "#261a33",
        bgB: "#100a17",
        accent: "#cfaee0",
        hasStitch: true,
        logoMode: "tinted",
        logoOpacity: 0.45,
        logoScale: 1.72,
        decor: `<g stroke="#cfaee0" fill="none" opacity="0.32">
          <circle cx="320" cy="198" r="136" stroke-width="1.3"/>
          <circle cx="320" cy="198" r="120" stroke-width="0.8"/>
          <polygon points="320,76 425,258 215,258" stroke-width="1.1"/>
          <polygon points="320,320 425,138 215,138" stroke-width="1.1"/>
          <rect x="234" y="112" width="172" height="172" stroke-width="0.7" opacity="0.5"/>
        </g>`,
      });
      break;
    case "ivory-cathedral": {
      const cathedralRose = [
        ...Array.from({ length: 12 }, (_, i) => {
          const deg = i * 30;
          return i % 2 === 0
            ? `<path d="M320 198 L309 124 Q320 96 331 124 Z" fill="#3a3630" opacity="0.24" transform="rotate(${deg} 320 198)"/>`
            : `<path d="M320 198 L311 112 Q320 82 329 112 Z" fill="none" stroke="#47423a" stroke-width="1.15" opacity="0.42" transform="rotate(${deg} 320 198)"/>`;
        }),
        `<circle cx="320" cy="198" r="130" fill="none" stroke="#4a453d" stroke-width="1.25" opacity="0.42"/>`,
        `<circle cx="320" cy="198" r="122" fill="none" stroke="#4a453d" stroke-width="0.65" stroke-dasharray="3 4" opacity="0.38"/>`,
        `<circle cx="320" cy="198" r="64" fill="#282521" stroke="#5c564b" stroke-width="1.6" opacity="0.94"/>`,
        `<circle cx="320" cy="198" r="57" fill="none" stroke="#dfd7c6" stroke-width="0.85" stroke-dasharray="2 3" opacity="0.58"/>`,
      ].join("");
      svg = makeBrandSvg({
        bgA: "#e6dfd0",
        bgB: "#c9bfa8",
        accent: "#595347",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "tinted",
        logoTint: "#efe8d8",
        logoOpacity: 0.94,
        logoScale: 1.45,
        decor: cathedralRose,
      });
      break;
    }
    case "tidal-cartography": {
      const rays = Array.from({ length: 24 }, (_, i) => {
        const a = (i * Math.PI) / 12;
        return `<line x1="320" y1="198" x2="${(320 + Math.cos(a) * 340).toFixed(1)}" y2="${(198 + Math.sin(a) * 340).toFixed(1)}"/>`;
      }).join("");
      svg = makeBrandSvg({
        bgA: "#14343b",
        bgB: "#09191e",
        accent: "#88ccc6",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "tinted",
        logoOpacity: 0.44,
        logoScale: 1.82,
        decor: `<g stroke="#88ccc6" fill="none" opacity="0.24">
          <g stroke-width="0.7">${rays}</g>
          <circle cx="320" cy="198" r="122" stroke-width="1.3"/>
          <polygon points="320,90 338,180 428,198 338,216 320,306 302,216 212,198 302,180" stroke-width="1.1"/>
        </g>`,
      });
      break;
    }
    case "ember-crucible": {
      const hexCells: string[] = [];
      for (let row = 0; row < 11; row++) {
        for (let col = 0; col < 17; col++) {
          const cx = col * 42 + (row % 2 ? 21 : 0);
          const cy = row * 38;
          const pts = starPoints(cx, cy, 20, 6)
            .map((p) => `${p[0].toFixed(1)},${p[1].toFixed(1)}`)
            .join(" ");
          hexCells.push(`<polygon points="${pts}"/>`);
        }
      }
      svg = makeBrandSvg({
        bgA: "#2b1d19",
        bgB: "#120b0a",
        accent: "#e38452",
        hasStitch: true,
        logoMode: "gold",
        logoOpacity: 0.8,
        logoScale: 1.95,
        decor: `<g stroke="#e38452" fill="none" stroke-width="0.75" opacity="0.16">${hexCells.join("")}</g>`,
      });
      break;
    }
    case "moonphase-archive": {
      const moons = (y: number) =>
        [-210, -140, -70, 0, 70, 140, 210]
          .map((dx, idx) => {
            const cx = 320 + dx;
            const cut = (idx - 3) * 4.5;
            return `<g opacity="0.62">
              <circle cx="${cx}" cy="${y}" r="11" fill="#d9e0f0"/>
              ${Math.abs(idx - 3) > 0 ? `<circle cx="${cx + cut}" cy="${y}" r="10.2" fill="#171226"/>` : ""}
            </g>`;
          })
          .join("");
      svg = makeBrandSvg({
        bgA: "#211b36",
        bgB: "#0f0c1a",
        accent: "#c5d0e8",
        hasStitch: true,
        logoMode: "silver",
        logoOpacity: 0.86,
        logoScale: 1.9,
        decor: `${moons(62)}${moons(338)}<line x1="82" y1="62" x2="558" y2="62" stroke="#c5d0e8" stroke-width="0.6" opacity="0.25"/><line x1="82" y1="338" x2="558" y2="338" stroke="#c5d0e8" stroke-width="0.6" opacity="0.25"/>`,
      });
      break;
    }
    case "heraldic-diamond": {
      const harlequin: string[] = [];
      for (let r = 0; r < 9; r++) {
        for (let c = 0; c < 15; c++) {
          const cx = c * 46;
          const cy = r * 48;
          harlequin.push(
            `<polygon points="${cx},${cy - 22} ${cx + 21},${cy} ${cx},${cy + 22} ${cx - 21},${cy}"/>`,
          );
        }
      }
      svg = makeBrandSvg({
        bgA: "#1b2838",
        bgB: "#0d141f",
        accent: "#d4b26a",
        cornerTicks: true,
        hasStitch: true,
        logoMode: "gold",
        logoOpacity: 0.82,
        logoScale: 1.95,
        decor: `<g stroke="#d4b26a" fill="none" stroke-width="0.7" opacity="0.13">${harlequin.join("")}</g>`,
      });
      break;
    }
    case "monolith-minimal":
      svg = makeBrandSvg({
        bgA: "#1f2228",
        bgB: "#13151a",
        accent: "#b2bcc9",
        borderDouble: false,
        borderOpacity: 0.25,
        hasStitch: false,
        showLogo: false,
        decor: officialSpellwakeLogoSvg(
          "mono",
          582,
          344,
          0.72,
          0.72,
          "tinted",
          "#c2cbd6",
        ),
      });
      break;
    default:
      return null;
  }

  BRAND_SVG_CACHE.set(slug, svg);
  return svg;
}

// ---------------------------------------------------------------------------
// Tactile Material Pixel Painters (14 Approved Finishes)
// ---------------------------------------------------------------------------
function paintPixelMat(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  fn: (
    u: number,
    v: number,
    px: number,
    py: number,
  ) => readonly [number, number, number],
  vigStrength = 0.34,
): void {
  const img = ctx.createImageData(w, h);
  const d = img.data;
  const maxX = Math.max(1, w - 1);
  const maxY = Math.max(1, h - 1);
  for (let py = 0; py < h; py++) {
    const v = py / maxY;
    for (let px = 0; px < w; px++) {
      const u = px / maxX;
      const [r, g, b] = fn(u, v, px, py);
      const i = (py * w + px) * 4;
      const dist = Math.hypot((u - 0.5) * 1.15, (v - 0.45) * 1.15);
      const vig = Math.max(0.76, 1.04 - dist * vigStrength);
      d[i] = Math.min(255, Math.max(0, Math.round(r * vig)));
      d[i + 1] = Math.min(255, Math.max(0, Math.round(g * vig)));
      d[i + 2] = Math.min(255, Math.max(0, Math.round(b * vig)));
      d[i + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
}

/** Paint any of the 14 procedural tactile material surfaces onto a 2D canvas. */
export function paintMaterialPlaymat(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  slug: string,
): boolean {
  switch (slug) {
    case "classic-casino-felt": {
      const n = makeNoise(201);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const g =
          (n.sample(px * 0.72, py * 0.72) - 0.5) * 18 +
          (n.fbm(u * 8, v * 5, 3) - 0.5) * 12;
        return [56 + g, 90 + g * 1.05, 66 + g * 0.9];
      });
      return true;
    }
    case "burgundy-billiard-baize": {
      const n = makeNoise(202);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const g =
          (n.sample(px * 0.68, py * 0.68) - 0.5) * 18 +
          (n.fbm(u * 7, v * 5, 3) - 0.5) * 14;
        return [102 + g * 1.1, 34 + g * 0.58, 44 + g * 0.62];
      });
      return true;
    }
    case "midnight-matte-neoprene": {
      const n = makeNoise(203);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const g =
          (n.sample(px * 0.92, py * 0.92) - 0.5) * 11 +
          (n.fbm(u * 9, v * 6, 3) - 0.5) * 8;
        return [26 + g, 42 + g, 70 + g * 1.08];
      });
      return true;
    }
    case "charcoal-heather-neoprene": {
      const n = makeNoise(204);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const fleck = n.sample(px * 0.85, py * 0.85);
        const h1 =
          (fleck > 0.74 ? 14 : fleck < 0.24 ? -8 : 0) +
          (n.fbm(u * 10, v * 7, 2) - 0.5) * 7;
        return [46 + h1, 49 + h1, 54 + h1];
      });
      return true;
    }
    case "obsidian-and-gold-marble": {
      const n = makeNoise(209);
      paintPixelMat(ctx, w, h, (u, v) => {
        const w1 = n.fbm(u * 5.2, v * 3.6, 4);
        const w2 = n.fbm(u * 9 + w1 * 3.2, v * 6.5 + w1 * 2.6, 4);
        const vein = Math.pow(
          Math.max(
            0,
            1 - Math.abs(Math.sin((u * 4.5 + v * 3.2 + w2 * 3.6) * Math.PI)) * 6.5,
          ),
          2.4,
        );
        const fine = Math.pow(
          Math.max(
            0,
            1 - Math.abs(Math.sin((u * 9 - v * 6 + w1 * 4) * Math.PI)) * 10,
          ),
          2.8,
        );
        const base = 22 + w1 * 14;
        return [
          base + vein * 176 + fine * 68,
          base + vein * 144 + fine * 60,
          base + 3 + vein * 70 + fine * 42,
        ];
      });
      return true;
    }
    case "carrara-white-marble": {
      const n = makeNoise(210);
      paintPixelMat(
        ctx,
        w,
        h,
        (u, v) => {
          const w1 = n.fbm(u * 4.5, v * 3.2, 4);
          const w2 = n.fbm(u * 8 + w1 * 2.8, v * 5.8 + w1 * 2.1, 4);
          const vein = Math.pow(
            Math.max(
              0,
              1 -
                Math.abs(Math.sin((u * 3.8 + v * 2.6 + w2 * 3.2) * Math.PI)) * 4.8,
            ),
            2.2,
          );
          const c = 230 - w1 * 16 - vein * 70;
          return [c, c + 1, c + 4];
        },
        0.22,
      );
      return true;
    }
    case "basalt-flagstone": {
      const n = makeNoise(222);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const macro = (n.fbm(u * 6, v * 4, 4) - 0.5) * 18;
        const spec = n.sample(px * 0.9, py * 0.9);
        const pit = spec < 0.08 ? -14 : spec > 0.94 ? 14 : (spec - 0.5) * 8;
        const c = 40 + macro + pit;
        return [c, c + 1, c + 3];
      });
      return true;
    }
    case "saddle-espresso-leather": {
      const n = makeNoise(211);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const peb = n.sample(px * 0.52, py * 0.52);
        const cre = Math.abs(peb - 0.5) < 0.08 ? -11 : (peb - 0.5) * 15;
        const pat = (n.fbm(u * 4, v * 3, 3) - 0.5) * 20;
        return [
          68 + pat + cre,
          45 + pat * 0.75 + cre * 0.75,
          31 + pat * 0.55 + cre * 0.55,
        ];
      });
      return true;
    }
    case "oxblood-library-leather": {
      const n = makeNoise(212);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const peb = (n.sample(px * 0.5, py * 0.5) - 0.5) * 14;
        const pat = (n.fbm(u * 3.5, v * 2.8, 3) - 0.5) * 22;
        return [
          84 + pat + peb,
          29 + pat * 0.45 + peb * 0.5,
          27 + pat * 0.45 + peb * 0.5,
        ];
      });
      return true;
    }
    case "obsidian-grain-leather": {
      const n = makeNoise(223);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const peb = (n.sample(px * 0.54, py * 0.54) - 0.5) * 12;
        const sheen =
          Math.exp(
            -Math.pow((u - 0.46) / 0.45, 2) - Math.pow((v - 0.4) / 0.45, 2),
          ) * 12;
        const c = 26 + peb + sheen;
        return [c, c + 1, c + 3];
      });
      return true;
    }
    case "woven-natural-linen": {
      const n = makeNoise(213);
      paintPixelMat(
        ctx,
        w,
        h,
        (u, v, px, py) => {
          const warp = Math.sin(px * 1.55) * 5;
          const weft = Math.sin(py * 1.55) * 5;
          const slub = (n.fbm(u * 14, v * 10, 3) - 0.5) * 15;
          const g = warp + weft + slub;
          return [196 + g, 183 + g * 0.96, 160 + g * 0.9];
        },
        0.25,
      );
      return true;
    }
    case "cartographers-vellum": {
      const n = makeNoise(214);
      paintPixelMat(
        ctx,
        w,
        h,
        (u, v) => {
          const m = n.fbm(u * 5, v * 3.5, 4);
          const f = n.fbm(u * 22, v * 16, 3);
          const fox = f > 0.74 ? (f - 0.74) * 95 : 0;
          const edge =
            Math.max(0, Math.hypot((u - 0.5) * 1.9, (v - 0.5) * 1.9) - 0.68) *
            85;
          return [
            216 - m * 22 - fox * 0.6 - edge,
            197 - m * 26 - fox * 0.85 - edge * 1.15,
            160 - m * 30 - fox - edge * 1.3,
          ];
        },
        0.22,
      );
      return true;
    }
    case "raw-indigo-canvas": {
      const n = makeNoise(218);
      paintPixelMat(ctx, w, h, (u, v, px, py) => {
        const weave = (Math.sin(px * 1.6) + Math.sin(py * 1.6)) * 3.5;
        const slub = (n.fbm(u * 11, v * 7, 3) - 0.5) * 14;
        const g = weave + slub;
        return [33 + g, 46 + g, 72 + g * 1.1];
      });
      return true;
    }
    case "charcoal-washi-paper": {
      const n = makeNoise(224);
      paintPixelMat(ctx, w, h, (u, v) => {
        const cloud = (n.fbm(u * 6, v * 4, 4) - 0.5) * 16;
        const f1 = n.fbm(u * 30 + v * 12, v * 28 - u * 10, 3);
        const fiber = f1 > 0.72 ? (f1 - 0.72) * 110 : 0;
        const c = 44 + cloud + fiber;
        return [c, c + 1, c + 3];
      });
      return true;
    }
    default:
      return false;
  }
}

// ---------------------------------------------------------------------------
// Unified 2D Canvas & Three.js CanvasTexture Compositor
// ---------------------------------------------------------------------------
export type PlaymatRenderOptions = {
  /** Base URL prefix where `playmats/<slug>.jpg` files are served (e.g. `import.meta.env.BASE_URL`). */
  assetBaseUrl?: string;
  /** Optional Playmat Effect layered over the playmat surface and edge (Clean bypasses). */
  effect?: PlaymatEffectSelection | undefined;
  /** Optional callback fired once an async SVG or raster artwork finishes painting. */
  onReady?: () => void;
};

const LIGHT_SURFACE_SLUGS = new Set([
  "archivists-vellum",
  "ivory-cathedral",
  "carrara-white-marble",
  "woven-natural-linen",
  "cartographers-vellum",
  "windswept-castle",
  "dragon-and-tiger",
  "under-the-wave-off-kanagawa",
  "marine",
  "boats-at-berck-sur-mer",
  "tiger-in-bamboo",
  "cliff-walk-at-pourville",
  "rocky-wooded-landscape",
  "landscape-with-castle-and-shepherds",
  "landscape-with-a-windmill",
]);

/** True when a playmat has a predominantly dark surface (guides effect contrast). */
export function isDarkPlaymatSurface(spec: PlaymatSpec): boolean {
  return !LIGHT_SURFACE_SLUGS.has(spec.slug);
}

const LOADED_IMAGE_CACHE = new Map<string, HTMLImageElement>();

function drawCoverImage(
  ctx: CanvasRenderingContext2D,
  img: HTMLImageElement,
  w: number,
  h: number,
): void {
  const iw = img.naturalWidth || img.width || w;
  const ih = img.naturalHeight || img.height || h;
  const targetRatio = w / h;
  const imgRatio = iw / ih;
  let sx = 0;
  let sy = 0;
  let sw = iw;
  let sh = ih;
  if (imgRatio > targetRatio) {
    sw = ih * targetRatio;
    sx = (iw - sw) / 2;
  } else {
    sh = iw / targetRatio;
    sy = (ih - sh) / 2;
  }
  ctx.drawImage(img, sx, sy, sw, sh, 0, 0, w, h);
}

function paintArtFallback(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  spec: PlaymatSpec,
): void {
  const grad = ctx.createLinearGradient(0, 0, w, h);
  grad.addColorStop(0, "#1d2633");
  grad.addColorStop(1, "#0f151e");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, w, h);
  drawPlaymatEdge(ctx, w, h, spec.edge);
}

function loadCachedImage(
  url: string,
  onLoad: (img: HTMLImageElement) => void,
): (() => void) | null {
  const cached = LOADED_IMAGE_CACHE.get(url);
  if (cached && cached.complete && cached.naturalWidth > 0) {
    onLoad(cached);
    return null;
  }
  if (typeof Image === "undefined") return null;
  let cancelled = false;
  const img = new Image();
  if (!url.startsWith("data:")) img.crossOrigin = "anonymous";
  img.onload = () => {
    LOADED_IMAGE_CACHE.set(url, img);
    if (!cancelled) onLoad(img);
  };
  img.src = url;
  return () => {
    cancelled = true;
  };
}

/**
 * Paint any of the 53 locked playmats (surface + custom edge treatment, zero surface
 * texture overlay) onto a 2D canvas. Returns a cleanup function that cancels pending
 * async image callbacks if the host unmounts.
 */
export function paintPlaymatCanvas(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  spec: PlaymatSpec,
  options: PlaymatRenderOptions = {},
): () => void {
  const applyEffect = () => {
    if (options.effect && options.effect.id !== "clean") {
      paintPlaymatEffectOverlay(
        ctx,
        w,
        h,
        options.effect.id,
        options.effect.seed,
        isDarkPlaymatSurface(spec),
      );
    }
  };

  if (spec.group === "material") {
    paintMaterialPlaymat(ctx, w, h, spec.slug);
    drawPlaymatEdge(ctx, w, h, spec.edge);
    applyEffect();
    options.onReady?.();
    return () => undefined;
  }

  if (spec.group === "brand") {
    const svg = buildBrandPlaymatSvg(spec.slug);
    if (!svg) {
      paintArtFallback(ctx, w, h, spec);
      applyEffect();
      return () => undefined;
    }
    const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
    paintArtFallback(ctx, w, h, spec);
    const cancel = loadCachedImage(url, (img) => {
      ctx.clearRect(0, 0, w, h);
      drawCoverImage(ctx, img, w, h);
      drawPlaymatEdge(ctx, w, h, spec.edge);
      applyEffect();
      options.onReady?.();
    });
    return cancel ?? (() => undefined);
  }

  // Raster artwork playmat served from the Spellwake asset package
  const base = options.assetBaseUrl ?? "/";
  const normalizedBase = base.endsWith("/") ? base : `${base}/`;
  const url = spec.artAssetPath ? `${normalizedBase}${spec.artAssetPath}` : "";
  paintArtFallback(ctx, w, h, spec);
  if (!url) return () => undefined;

  const cancel = loadCachedImage(url, (img) => {
    ctx.clearRect(0, 0, w, h);
    drawCoverImage(ctx, img, w, h);
    drawPlaymatEdge(ctx, w, h, spec.edge);
    applyEffect();
    options.onReady?.();
  });
  return cancel ?? (() => undefined);
}

/**
 * Create a Three.js `CanvasTexture` for any locked playmat at 1024x640 resolution.
 * Automatically flags `needsUpdate` and calls `onReady` when async art/SVG paints.
 */
export function createPlaymatThreeTexture(
  spec: PlaymatSpec,
  options: PlaymatRenderOptions = {},
): { texture: THREE.CanvasTexture; cancel: () => void } {
  const width = 1024;
  const height = 640;
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  const ctx = canvas.getContext("2d");
  if (!ctx) return { texture, cancel: () => undefined };

  const cancel = paintPlaymatCanvas(ctx, width, height, spec, {
    ...options,
    onReady: () => {
      texture.needsUpdate = true;
      options.onReady?.();
    },
  });
  texture.needsUpdate = true;
  return { texture, cancel };
}
