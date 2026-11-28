import { describe, expect, test } from "bun:test";
import {
  PLAYMAT_EFFECTS,
  PLAYMAT_EFFECT_SEED_MAX,
  PLAYMAT_EFFECT_SEED_MIN,
  normalizePlaymatEffectSeed,
  playmatEffectSpec,
  resolvePlaymatEffect,
} from "./playmat-effects";

describe("playmat effects catalog", () => {
  test("defines Clean (free default) plus all 7 locked paid effects", () => {
    expect(PLAYMAT_EFFECTS.map((e) => e.id)).toEqual([
      "clean",
      "dusty",
      "oil_stained",
      "frayed",
      "creased",
      "drink_spill",
      "lgs_grime",
      "scorched",
    ]);
  });

  test("grants only Clean free and prices every paid effect between $0.49 and $1.49", () => {
    const free = PLAYMAT_EFFECTS.filter((e) => e.isFree);
    const paid = PLAYMAT_EFFECTS.filter((e) => !e.isFree);
    expect(free.map((e) => e.id)).toEqual(["clean"]);
    expect(free[0]?.priceCents).toBe(0);
    expect(free[0]?.priceLabel).toBeNull();
    expect(paid).toHaveLength(7);
    for (const fx of paid) {
      expect(fx.priceCents).toBeGreaterThanOrEqual(49);
      expect(fx.priceCents).toBeLessThanOrEqual(149);
      expect(fx.priceLabel).toMatch(/^\$(0\.49|0\.99|1\.49)$/);
    }
  });

  test("has unique canonical UUIDs, preview IDs, and asset refs", () => {
    expect(new Set(PLAYMAT_EFFECTS.map((e) => e.canonicalId)).size).toBe(8);
    expect(new Set(PLAYMAT_EFFECTS.map((e) => e.previewId)).size).toBe(8);
    expect(new Set(PLAYMAT_EFFECTS.map((e) => e.assetRef)).size).toBe(8);
  });

  test("resolves effects by ID, canonical UUID, preview ID, and asset ref", () => {
    const oil = resolvePlaymatEffect("oil_stained");
    expect(oil?.name).toBe("Oil Stained");
    expect(resolvePlaymatEffect(oil!.canonicalId)).toBe(oil);
    expect(resolvePlaymatEffect(oil!.previewId)).toBe(oil);
    expect(resolvePlaymatEffect(oil!.assetRef)).toBe(oil);
    expect(resolvePlaymatEffect(null)).toBeNull();
    expect(playmatEffectSpec("lgs_grime").description).toContain("bottom-left");
  });

  test("normalizes pattern seeds into the bounded deterministic range", () => {
    expect(normalizePlaymatEffectSeed(undefined, 347)).toBe(347);
    expect(normalizePlaymatEffectSeed(50, 347)).toBe(347);
    expect(normalizePlaymatEffectSeed(1200, 347)).toBe(347);
    expect(normalizePlaymatEffectSeed(PLAYMAT_EFFECT_SEED_MIN, 347)).toBe(
      PLAYMAT_EFFECT_SEED_MIN,
    );
    expect(normalizePlaymatEffectSeed(PLAYMAT_EFFECT_SEED_MAX, 347)).toBe(
      PLAYMAT_EFFECT_SEED_MAX,
    );
  });
});
