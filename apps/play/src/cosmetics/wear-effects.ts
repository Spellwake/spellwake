import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import {
  CARD_WEAR_TIERS,
  SLEEVE_WEAR_TIERS,
  cardWearTierSpec,
  normalizeWearSeed,
  sleeveWearTierSpec,
  WEAR_SEED_MAX,
  WEAR_SEED_MIN,
  type WearTierId,
} from "spellwake-renderer-benchmark/wear-effects";

export type WearTarget = "card" | "sleeve";

export type WearEffectCatalogEntry = {
  readonly canonicalId: string;
  readonly previewId: string;
  readonly assetRef: string;
  readonly target: WearTarget;
  readonly kind: "card_effect" | "sleeve_effect";
  readonly tier: WearTierId;
  readonly name: string;
  readonly isFree: boolean;
  readonly priceCents: number;
  readonly priceLabel: string | null;
  readonly defaultSeed: number;
};

/** Paid condition tiers are priced at $2.99 (299 cents); Near Mint and Lightly Played are free. */
export const WEAR_EFFECT_PRICE_CENTS = 299;
export const WEAR_EFFECT_PRICE_LABEL = "$2.99";

export const CARD_EFFECT_CANONICAL_IDS: Readonly<Record<WearTierId, string>> = {
  nm: "e0000001-0001-4000-8000-000000000001",
  lp: "e0000001-0001-4000-8000-000000000002",
  mp: "e0000001-0001-4000-8000-000000000003",
  hp: "e0000001-0001-4000-8000-000000000004",
  dmg: "e0000001-0001-4000-8000-000000000005",
  unplayable: "e0000001-0001-4000-8000-000000000006",
};

export const SLEEVE_EFFECT_CANONICAL_IDS: Readonly<Record<WearTierId, string>> =
  {
    nm: "e0000002-0001-4000-8000-000000000001",
    lp: "e0000002-0001-4000-8000-000000000002",
    mp: "e0000002-0001-4000-8000-000000000003",
    hp: "e0000002-0001-4000-8000-000000000004",
    dmg: "e0000002-0001-4000-8000-000000000005",
    unplayable: "e0000002-0001-4000-8000-000000000006",
  };

export const PREVIEW_CARD_EFFECT_IDS: Readonly<Record<WearTierId, string>> = {
  nm: "preview-card_effect-nm",
  lp: "preview-card_effect-lp",
  mp: "preview-card_effect-mp",
  hp: "preview-card_effect-hp",
  dmg: "preview-card_effect-dmg",
  unplayable: "preview-card_effect-unplayable",
};

export const PREVIEW_SLEEVE_EFFECT_IDS: Readonly<Record<WearTierId, string>> = {
  nm: "preview-sleeve_effect-nm",
  lp: "preview-sleeve_effect-lp",
  mp: "preview-sleeve_effect-mp",
  hp: "preview-sleeve_effect-hp",
  dmg: "preview-sleeve_effect-dmg",
  unplayable: "preview-sleeve_effect-unplayable",
};

const isFreeTier = (tier: WearTierId): boolean =>
  tier === "nm" || tier === "lp";

export const CARD_EFFECT_CATALOG: readonly WearEffectCatalogEntry[] =
  CARD_WEAR_TIERS.map((spec) => {
    const free = isFreeTier(spec.id);
    return {
      canonicalId: CARD_EFFECT_CANONICAL_IDS[spec.id],
      previewId: PREVIEW_CARD_EFFECT_IDS[spec.id],
      assetRef: `effect:card:${spec.id}`,
      target: "card",
      kind: "card_effect",
      tier: spec.id,
      name: spec.label,
      isFree: free,
      priceCents: free ? 0 : WEAR_EFFECT_PRICE_CENTS,
      priceLabel: free ? null : WEAR_EFFECT_PRICE_LABEL,
      defaultSeed: spec.defaultSeed,
    };
  });

export const SLEEVE_EFFECT_CATALOG: readonly WearEffectCatalogEntry[] =
  SLEEVE_WEAR_TIERS.map((spec) => {
    const free = isFreeTier(spec.id);
    return {
      canonicalId: SLEEVE_EFFECT_CANONICAL_IDS[spec.id],
      previewId: PREVIEW_SLEEVE_EFFECT_IDS[spec.id],
      assetRef: `effect:sleeve:${spec.id}`,
      target: "sleeve",
      kind: "sleeve_effect",
      tier: spec.id,
      name: spec.label,
      isFree: free,
      priceCents: free ? 0 : WEAR_EFFECT_PRICE_CENTS,
      priceLabel: free ? null : WEAR_EFFECT_PRICE_LABEL,
      defaultSeed: spec.defaultSeed,
    };
  });

export const ALL_WEAR_EFFECTS: readonly WearEffectCatalogEntry[] = [
  ...CARD_EFFECT_CATALOG,
  ...SLEEVE_EFFECT_CATALOG,
];

const ENTRY_BY_KEY = new Map<string, WearEffectCatalogEntry>();
for (const entry of ALL_WEAR_EFFECTS) {
  ENTRY_BY_KEY.set(entry.canonicalId, entry);
  ENTRY_BY_KEY.set(entry.previewId, entry);
  ENTRY_BY_KEY.set(entry.assetRef, entry);
}

/** Resolve any canonical UUID, preview ID, or asset ref back to its wear entry. */
export function resolveWearEffect(
  idOrRef: string | null | undefined,
): WearEffectCatalogEntry | null {
  if (!idOrRef) return null;
  return ENTRY_BY_KEY.get(idOrRef) ?? null;
}

/** Resolve a saved cosmetic ID to a wear tier, defaulting cleanly to Near Mint. */
export function wearTierOf(
  idOrRef: string | null | undefined,
  expectedTarget: WearTarget,
): WearTierId {
  const entry = resolveWearEffect(idOrRef);
  if (!entry || entry.target !== expectedTarget) return "nm";
  return entry.tier;
}

export type WearSeedMap = Readonly<Record<string, number>>;

const wearSeedsAtom = atomWithStorage<WearSeedMap>(
  "spellwake:cosmetics:wear-seeds:v1",
  {},
);

export type ConnectedWearDefaults = {
  cardEffectId: string;
  sleeveEffectId: string;
};

export const connectedWearDefaultsAtom = atomWithStorage<ConnectedWearDefaults>(
  "spellwake:cosmetics:connected:wear-defaults:v1",
  {
    cardEffectId: CARD_EFFECT_CANONICAL_IDS.nm,
    sleeveEffectId: SLEEVE_EFFECT_CANONICAL_IDS.nm,
  },
);

const seedKey = (target: WearTarget, tier: WearTierId): string =>
  `${target}:${tier}`;

export function defaultSeedFor(target: WearTarget, tier: WearTierId): number {
  return target === "card"
    ? cardWearTierSpec(tier).defaultSeed
    : sleeveWearTierSpec(tier).defaultSeed;
}

function nextRandomSeed(current: number): number {
  const span = WEAR_SEED_MAX - WEAR_SEED_MIN + 1;
  let candidate = WEAR_SEED_MIN + Math.floor(Math.random() * span);
  if (candidate === current) {
    candidate = ((candidate - WEAR_SEED_MIN + 137) % span) + WEAR_SEED_MIN;
  }
  return candidate;
}

/** Read and reroll per-tier wear pattern seeds shared across cosmetics preview and live matches. */
export function useWearSeeds(): {
  seedFor: (target: WearTarget, tier: WearTierId) => number;
  isCustomSeed: (target: WearTarget, tier: WearTierId) => boolean;
  randomizeSeed: (target: WearTarget, tier: WearTierId) => number;
  resetSeed: (target: WearTarget, tier: WearTierId) => void;
} {
  const [seeds, setSeeds] = useAtom(wearSeedsAtom);

  const seedFor = (target: WearTarget, tier: WearTierId): number => {
    const fallback = defaultSeedFor(target, tier);
    if (tier === "nm") return 0;
    return normalizeWearSeed(seeds[seedKey(target, tier)], fallback);
  };

  const isCustomSeed = (target: WearTarget, tier: WearTierId): boolean => {
    if (tier === "nm") return false;
    const saved = seeds[seedKey(target, tier)];
    return saved !== undefined && saved !== defaultSeedFor(target, tier);
  };

  const randomizeSeed = (target: WearTarget, tier: WearTierId): number => {
    if (tier === "nm") return 0;
    const next = nextRandomSeed(seedFor(target, tier));
    setSeeds((prev) => ({ ...prev, [seedKey(target, tier)]: next }));
    return next;
  };

  const resetSeed = (target: WearTarget, tier: WearTierId): void => {
    if (tier === "nm") return;
    setSeeds((prev) => {
      const key = seedKey(target, tier);
      if (!(key in prev)) return prev;
      const copy = { ...prev };
      delete copy[key];
      return copy;
    });
  };

  return { seedFor, isCustomSeed, randomizeSeed, resetSeed };
}
