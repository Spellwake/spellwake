import { describe, expect, test } from "bun:test";
import {
  CARD_WEAR_TIERS,
  SLEEVE_WEAR_TIERS,
  cardWearTierSpec,
  createCardWearProfile,
  createCardWearTextures,
  createNestedSleeveStackPlate,
  createSleeveWearProfile,
  createSleeveWearTextures,
  createWornSleeveGeometries,
  normalizeWearSeed,
  sleeveWearTierSpec,
} from "./wear-effects";

describe("wear-effects canonical tiers, seeds, and 3D geometries", () => {
  test("locked-in card and sleeve tier ladders keep Near Mint bypassed and canonical seeds", () => {
    expect(CARD_WEAR_TIERS.map((t) => [t.id, t.defaultSeed])).toEqual([
      ["nm", 0],
      ["lp", 329],
      ["mp", 751],
      ["hp", 456],
      ["dmg", 895],
      ["unplayable", 985],
    ]);
    expect(cardWearTierSpec("nm")).toMatchObject({ wear: 0, bend: 0 });
    expect(sleeveWearTierSpec("nm")).toMatchObject({
      cornerBend: 0,
      grimeHaze: 0,
      lipWave: 0,
    });
    expect(SLEEVE_WEAR_TIERS.map((t) => [t.id, t.defaultSeed])).toEqual([
      ["nm", 0],
      ["lp", 141],
      ["mp", 317],
      ["hp", 523],
      ["dmg", 739],
      ["unplayable", 947],
    ]);
  });

  test("normalizeWearSeed clamps invalid values back to the tier default", () => {
    expect(normalizeWearSeed(undefined, 329)).toBe(329);
    expect(normalizeWearSeed(0, 329)).toBe(329);
    expect(normalizeWearSeed(1000, 329)).toBe(329);
    expect(normalizeWearSeed(Number.NaN, 329)).toBe(329);
    expect(normalizeWearSeed(412.4, 329)).toBe(412);
  });

  test("card and sleeve wear texture generators produce deterministic finite RGBA buffers", () => {
    const cProf = createCardWearProfile(456);
    const cTex = createCardWearTextures(cProf, false, 64, 90);
    expect(cTex.maskTex.image.data.length).toBe(64 * 90 * 4);
    expect(cTex.normalTex.image.data.length).toBe(64 * 90 * 4);
    cTex.dispose();

    const sProf = createSleeveWearProfile(523);
    const sTex = createSleeveWearTextures(sProf, false, 64, 90);
    expect(sTex.maskTex.image.data.length).toBe(64 * 90 * 4);
    expect(sTex.normalTex.image.data.length).toBe(64 * 90 * 4);
    sTex.dispose();
  });

  test("worn sleeve and nested stack-plate geometries build finite vertices for all tiers", () => {
    const profile = createSleeveWearProfile(947);
    const boardSleeve = createWornSleeveGeometries(
      profile,
      1.0,
      1.0,
      false,
      16,
      20,
    );
    const deckSleeve = createWornSleeveGeometries(
      profile,
      1.0,
      1.0,
      true,
      16,
      20,
    );
    const stackPlate = createNestedSleeveStackPlate(
      profile,
      1.0,
      1.0,
      0.017,
      12,
      16,
    );

    for (const geo of [
      boardSleeve.frontFilm,
      boardSleeve.backFace,
      boardSleeve.seam,
      deckSleeve.frontFilm,
      deckSleeve.backFace,
      deckSleeve.seam,
      stackPlate,
    ]) {
      const pos = geo.getAttribute("position");
      expect(pos.count).toBeGreaterThan(0);
      for (let i = 0; i < pos.count; i++) {
        expect(Number.isFinite(pos.getX(i))).toBe(true);
        expect(Number.isFinite(pos.getY(i))).toBe(true);
        expect(Number.isFinite(pos.getZ(i))).toBe(true);
      }
    }
    boardSleeve.dispose();
    deckSleeve.dispose();
    stackPlate.dispose();
  });
});
