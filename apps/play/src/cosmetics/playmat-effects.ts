import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import {
  PLAYMAT_EFFECTS,
  PLAYMAT_EFFECT_SEED_MAX,
  PLAYMAT_EFFECT_SEED_MIN,
  normalizePlaymatEffectSeed,
  playmatEffectSpec,
  resolvePlaymatEffect,
  type PlaymatEffectId,
} from "spellwake-renderer-benchmark/playmat-effects";

export { PLAYMAT_EFFECTS, resolvePlaymatEffect, type PlaymatEffectId };

export const PLAYMAT_EFFECT_CANONICAL_IDS: Readonly<
  Record<PlaymatEffectId, string>
> = {
  clean: "e0000004-0001-4000-8000-000000000001",
  dusty: "e0000004-0001-4000-8000-000000000002",
  oil_stained: "e0000004-0001-4000-8000-000000000003",
  frayed: "e0000004-0001-4000-8000-000000000004",
  creased: "e0000004-0001-4000-8000-000000000005",
  drink_spill: "e0000004-0001-4000-8000-000000000006",
  lgs_grime: "e0000004-0001-4000-8000-000000000007",
  scorched: "e0000004-0001-4000-8000-000000000008",
};

export const PREVIEW_PLAYMAT_EFFECT_IDS: Readonly<
  Record<PlaymatEffectId, string>
> = {
  clean: "preview-playmat_effect-clean",
  dusty: "preview-playmat_effect-dusty",
  oil_stained: "preview-playmat_effect-oil_stained",
  frayed: "preview-playmat_effect-frayed",
  creased: "preview-playmat_effect-creased",
  drink_spill: "preview-playmat_effect-drink_spill",
  lgs_grime: "preview-playmat_effect-lgs_grime",
  scorched: "preview-playmat_effect-scorched",
};

/** Resolve any saved playmat effect ID to a PlaymatEffectId, defaulting cleanly to Clean. */
export function playmatEffectIdOf(
  idOrRef: string | null | undefined,
): PlaymatEffectId {
  return resolvePlaymatEffect(idOrRef)?.id ?? "clean";
}

export type PlaymatEffectSeedMap = Readonly<Record<string, number>>;

const playmatEffectSeedsAtom = atomWithStorage<PlaymatEffectSeedMap>(
  "spellwake:cosmetics:playmat-effect-seeds:v1",
  {},
);

export const connectedPlaymatEffectDefaultAtom = atomWithStorage<string>(
  "spellwake:cosmetics:connected:playmat-effect-default:v1",
  PLAYMAT_EFFECT_CANONICAL_IDS.clean,
);

function nextRandomSeed(current: number): number {
  const span = PLAYMAT_EFFECT_SEED_MAX - PLAYMAT_EFFECT_SEED_MIN + 1;
  let candidate = PLAYMAT_EFFECT_SEED_MIN + Math.floor(Math.random() * span);
  if (candidate === current) {
    candidate =
      ((candidate - PLAYMAT_EFFECT_SEED_MIN + 137) % span) +
      PLAYMAT_EFFECT_SEED_MIN;
  }
  return candidate;
}

/**
 * Persisted per-effect pattern seeds shared across the Cosmetics preview and live matches.
 * Once randomized, a playmat effect keeps that exact visual layout until randomized again.
 */
export function usePlaymatEffectSeeds(): {
  seedFor: (effectId: PlaymatEffectId) => number;
  isCustomSeed: (effectId: PlaymatEffectId) => boolean;
  randomizeSeed: (effectId: PlaymatEffectId) => number;
  resetSeed: (effectId: PlaymatEffectId) => void;
} {
  const [seeds, setSeeds] = useAtom(playmatEffectSeedsAtom);

  const seedFor = (effectId: PlaymatEffectId): number => {
    if (effectId === "clean") return 0;
    const fallback = playmatEffectSpec(effectId).defaultSeed;
    return normalizePlaymatEffectSeed(seeds[effectId], fallback);
  };

  const isCustomSeed = (effectId: PlaymatEffectId): boolean => {
    if (effectId === "clean") return false;
    const saved = seeds[effectId];
    return saved !== undefined && saved !== playmatEffectSpec(effectId).defaultSeed;
  };

  const randomizeSeed = (effectId: PlaymatEffectId): number => {
    if (effectId === "clean") return 0;
    const next = nextRandomSeed(seedFor(effectId));
    setSeeds((prev) => ({ ...prev, [effectId]: next }));
    return next;
  };

  const resetSeed = (effectId: PlaymatEffectId): void => {
    if (effectId === "clean") return;
    setSeeds((prev) => {
      if (!(effectId in prev)) return prev;
      const copy = { ...prev };
      delete copy[effectId];
      return copy;
    });
  };

  return { seedFor, isCustomSeed, randomizeSeed, resetSeed };
}
