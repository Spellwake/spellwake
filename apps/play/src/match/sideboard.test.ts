import { describe, expect, test } from "bun:test";
import {
  changed,
  counts,
  moveCard,
  samePool,
  sideboardCards,
  withSideboard,
} from "./sideboard";

const BOLT = "11111111-1111-4111-8111-111111111111";
const ABRADE = "22222222-2222-4222-8222-222222222222";
const SHOCK = "33333333-3333-4333-8333-333333333333";
const PRINT = "44444444-4444-4444-8444-444444444444";
const names: Record<string, string> = {
  [BOLT]: "Lightning Bolt",
  [ABRADE]: "Abrade",
  [SHOCK]: "Shock",
};
const nameOf = (id: string) => names[id];

const entries = [
  { definitionId: BOLT, printingId: PRINT, section: "main", quantity: "4" },
  { definitionId: SHOCK, printingId: null, section: "main", quantity: "2" },
  {
    definitionId: SHOCK,
    printingId: null,
    section: "sideboard",
    quantity: "2",
  },
  {
    definitionId: ABRADE,
    printingId: null,
    section: "sideboard",
    quantity: "3",
  },
  {
    definitionId: ABRADE,
    printingId: null,
    section: "considering",
    quantity: "1",
  },
];

describe("a deck's main deck and sideboard", () => {
  const cards = sideboardCards(entries, nameOf);

  test("read as one row per card, by name, without the builder's other sections", () => {
    expect(cards).toEqual([
      { key: `${ABRADE}/`, name: "Abrade", main: 0, side: 3 },
      { key: `${BOLT}/${PRINT}`, name: "Lightning Bolt", main: 4, side: 0 },
      { key: `${SHOCK}/`, name: "Shock", main: 2, side: 2 },
    ]);
    expect(counts(cards)).toEqual({ main: 6, side: 5 });
  });

  test("move one copy at a time, and never below none", () => {
    const moved = moveCard(
      moveCard(cards, `${ABRADE}/`, "main"),
      `${BOLT}/${PRINT}`,
      "side",
    );
    expect(counts(moved)).toEqual({ main: 6, side: 5 });
    expect(moved.find((card) => card.name === "Abrade")).toMatchObject({
      main: 1,
      side: 2,
    });
    expect(changed(cards, moved)).toBe(true);
    expect(moveCard(cards, `${BOLT}/${PRINT}`, "main")).toEqual(cards);
    expect(changed(cards, cards)).toBe(false);
  });

  test("save as entries that keep every other section and the same cards", () => {
    const moved = moveCard(cards, `${ABRADE}/`, "main");
    const saved = withSideboard(entries, moved);
    expect(saved).toContainEqual({
      definitionId: ABRADE,
      printingId: null,
      section: "considering",
      quantity: "1",
    });
    expect(saved).toContainEqual({
      definitionId: ABRADE,
      printingId: null,
      section: "main",
      quantity: "1",
    });
    expect(samePool(entries, saved)).toBe(true);
  });
});

describe("the same cards", () => {
  test("is false once a card is added or taken out", () => {
    expect(
      samePool(entries, [
        ...entries,
        {
          definitionId: BOLT,
          printingId: PRINT,
          section: "sideboard",
          quantity: "1",
        },
      ]),
    ).toBe(false);
    expect(samePool(entries, entries.slice(1))).toBe(false);
  });
});
