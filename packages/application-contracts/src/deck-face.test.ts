import { describe, expect, test } from "bun:test";
import { changeDeckSettingsSchema, deckSummarySchema } from "./index.ts";

const id = (n: number): string =>
  `${String(n).padStart(8, "0")}-0000-4000-8000-000000000000`;
const settings = {
  name: "Goblins",
  mode: "collection",
  formatId: id(1),
  appearance: {
    backId: id(2),
    sleeveId: id(3),
    diceId: id(4),
    playmatId: null,
  },
} as const;
const summary = {
  id: id(5),
  name: "Goblins",
  mode: "collection",
  currentRevisionId: id(6),
  format: { id: id(1), key: "standard", name: "Standard" },
  appearance: settings.appearance,
};

describe("deck face cards", () => {
  test("a change may name a face card; the observed baseline may not", () => {
    const change = {
      requestId: id(7),
      deckId: id(5),
      expected: settings,
      settings: { ...settings, faceCardId: id(8) },
    };
    expect(changeDeckSettingsSchema.safeParse(change).success).toBe(true);
    expect(
      changeDeckSettingsSchema.safeParse({
        ...change,
        expected: { ...settings, faceCardId: id(8) },
      }).success,
    ).toBe(false);
    expect(
      changeDeckSettingsSchema.safeParse({
        ...change,
        settings: { ...settings, faceCardId: null },
      }).success,
    ).toBe(false);
  });
  test("summaries carry the face card, accept older servers and refuse non-https art", () => {
    const art = `https://cards.scryfall.io/art_crop/front/0/0/${id(9)}.jpg`;
    expect(
      deckSummarySchema.safeParse({
        ...summary,
        faceCard: { definitionId: id(9), artUrl: art },
      }).success,
    ).toBe(true);
    expect(
      deckSummarySchema.safeParse({ ...summary, faceCard: null }).success,
    ).toBe(true);
    expect(deckSummarySchema.safeParse(summary).success).toBe(true);
    expect(
      deckSummarySchema.safeParse({
        ...summary,
        faceCard: { definitionId: id(9), artUrl: "private:catalogue-art" },
      }).success,
    ).toBe(false);
  });
});
