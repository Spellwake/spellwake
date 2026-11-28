import { describe, expect, test } from "bun:test";
import {
  CARD_EFFECT_CANONICAL_IDS,
  CARD_EFFECT_CATALOG,
  PREVIEW_CARD_EFFECT_IDS,
  PREVIEW_SLEEVE_EFFECT_IDS,
  SLEEVE_EFFECT_CANONICAL_IDS,
  SLEEVE_EFFECT_CATALOG,
  WEAR_EFFECT_PRICE_CENTS,
  WEAR_EFFECT_PRICE_LABEL,
  defaultSeedFor,
  resolveWearEffect,
  wearTierOf,
} from "./wear-effects";

describe("card & sleeve wear effects catalogue", () => {
  test("Near Mint and Lightly Played are free; MP, HP, Damaged, and Unplayable are $2.99", () => {
    for (const catalog of [CARD_EFFECT_CATALOG, SLEEVE_EFFECT_CATALOG]) {
      expect(catalog).toHaveLength(6);
      const byTier = new Map(catalog.map((e) => [e.tier, e]));
      expect(byTier.get("nm")?.isFree).toBe(true);
      expect(byTier.get("nm")?.priceCents).toBe(0);
      expect(byTier.get("nm")?.priceLabel).toBeNull();
      expect(byTier.get("lp")?.isFree).toBe(true);
      expect(byTier.get("lp")?.priceCents).toBe(0);
      expect(byTier.get("lp")?.priceLabel).toBeNull();

      for (const paidTier of ["mp", "hp", "dmg", "unplayable"] as const) {
        const entry = byTier.get(paidTier);
        expect(entry?.isFree).toBe(false);
        expect(entry?.priceCents).toBe(WEAR_EFFECT_PRICE_CENTS);
        expect(entry?.priceCents).toBe(299);
        expect(entry?.priceLabel).toBe(WEAR_EFFECT_PRICE_LABEL);
      }
    }
  });

  test("resolves canonical UUIDs, preview IDs, and asset refs cleanly and defaults unknown IDs to Near Mint", () => {
    expect(resolveWearEffect(CARD_EFFECT_CANONICAL_IDS.hp)?.tier).toBe("hp");
    expect(resolveWearEffect(PREVIEW_CARD_EFFECT_IDS.dmg)?.tier).toBe("dmg");
    expect(resolveWearEffect("effect:sleeve:unplayable")?.tier).toBe(
      "unplayable",
    );
    expect(wearTierOf(SLEEVE_EFFECT_CANONICAL_IDS.lp, "sleeve")).toBe("lp");
    expect(wearTierOf(PREVIEW_SLEEVE_EFFECT_IDS.mp, "sleeve")).toBe("mp");
    // Cross-target or missing IDs safely fall back to Near Mint.
    expect(wearTierOf(CARD_EFFECT_CANONICAL_IDS.hp, "sleeve")).toBe("nm");
    expect(wearTierOf(null, "card")).toBe("nm");
    expect(wearTierOf("unknown-id", "card")).toBe("nm");
  });

  test("preserves locked-in default pattern seeds", () => {
    expect(defaultSeedFor("card", "lp")).toBe(329);
    expect(defaultSeedFor("card", "mp")).toBe(751);
    expect(defaultSeedFor("card", "hp")).toBe(456);
    expect(defaultSeedFor("card", "dmg")).toBe(895);
    expect(defaultSeedFor("card", "unplayable")).toBe(985);
    expect(defaultSeedFor("sleeve", "lp")).toBe(141);
    expect(defaultSeedFor("sleeve", "mp")).toBe(317);
    expect(defaultSeedFor("sleeve", "hp")).toBe(523);
    expect(defaultSeedFor("sleeve", "dmg")).toBe(739);
    expect(defaultSeedFor("sleeve", "unplayable")).toBe(947);
  });
});
