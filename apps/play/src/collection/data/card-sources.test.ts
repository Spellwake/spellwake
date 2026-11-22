import { describe, expect, test } from "bun:test";
import {
  activeFilterCount,
  matchCards,
  NEUTRAL_QUERY,
  orderPrintings,
  type OwnedCard,
  type OwnedPrinting,
} from "../card-sources";

const printing = (
  id: string,
  setCode: string,
  copies: number,
  rarity: OwnedPrinting["rarity"] = null,
  finish = "Nonfoil",
): OwnedPrinting => ({
  id,
  setCode,
  setName: setCode,
  number: "1",
  finish,
  rarity,
  copies,
  faceUrl: null,
  backUrl: null,
});
const card = (
  id: string,
  name: string,
  printings: OwnedPrinting[],
  extra: Partial<OwnedCard> = {},
): OwnedCard => ({
  id,
  name,
  copies: printings.reduce((sum, p) => sum + p.copies, 0),
  printings: orderPrintings(printings),
  type: "Instant",
  rules: null,
  manaValue: 1,
  colors: ["R"],
  identity: ["R"],
  legality: { modern: "legal" },
  ...extra,
});

const bolt = card("bolt", "Lightning Bolt", [
  printing("bolt-m11", "M11", 4, "common"),
  printing("bolt-2x2", "2X2", 2, "uncommon"),
  printing("bolt-2x2f", "2X2", 1, "uncommon", "Foil"),
]);
const island = card(
  "island",
  "Island",
  [printing("island", "M21", 20, "common")],
  {
    type: "Basic Land — Island",
    manaValue: 0,
    colors: [],
    identity: ["U"],
  },
);
const counter = card(
  "counter",
  "Counterspell",
  [printing("cs", "MH2", 2, "uncommon")],
  {
    manaValue: 2,
    colors: ["U"],
    identity: ["U"],
    rules: "Counter target spell.",
    legality: { modern: "not_legal" },
  },
);
const unknown = card("mystery", "Mystery", [printing("m", "XYZ", 3)], {
  type: null,
  manaValue: null,
  colors: null,
  identity: null,
  legality: null,
});
const all = [bolt, island, counter, unknown];
const names = (query: Partial<typeof NEUTRAL_QUERY>) =>
  matchCards(all, { ...NEUTRAL_QUERY, ...query }).map((m) => m.card.name);

describe("matchCards", () => {
  test("searches names, types and rules text", () => {
    expect(names({ search: "counter target" })).toEqual(["Counterspell"]);
    expect(names({ search: "island" })).toEqual(["Island"]);
  });
  test("format keeps only cards legal in it", () => {
    expect(names({ format: "Modern" })).toEqual(["Island", "Lightning Bolt"]);
  });
  test("set and rarity must hold for the same printing, which then represents the card", () => {
    const [match] = matchCards(all, {
      ...NEUTRAL_QUERY,
      sets: ["2X2"],
      rarities: ["uncommon"],
    });
    expect(match?.card.name).toBe("Lightning Bolt");
    expect(match?.printing.id).toBe("bolt-2x2");
    expect(names({ sets: ["M11"], rarities: ["uncommon"] })).toEqual([]);
  });
  test("colorless alone is exact; with a colour it is an alternative", () => {
    expect(names({ colorless: true })).toEqual(["Island"]);
    expect(names({ colorless: true, colors: ["R"] })).toEqual([
      "Island",
      "Lightning Bolt",
    ]);
    expect(names({ colors: ["U"], colorMode: "identity" })).toEqual([
      "Counterspell",
      "Island",
    ]);
  });
  test("an unknown value never matches a filter on it", () => {
    expect(names({ types: ["Instant"] })).not.toContain("Mystery");
    expect(names({ manaMin: 0 })).not.toContain("Mystery");
    expect(names({ colors: ["R"] })).not.toContain("Mystery");
  });
  test("mana value bounds are inclusive", () => {
    expect(names({ manaMin: 1, manaMax: 2 })).toEqual([
      "Counterspell",
      "Lightning Bolt",
    ]);
  });
  test("sorts by owned copies and by mana value, unknowns last", () => {
    expect(names({ sort: "owned-desc" })).toEqual([
      "Island",
      "Lightning Bolt",
      "Mystery",
      "Counterspell",
    ]);
    expect(names({ sort: "mana-desc" })).toEqual([
      "Counterspell",
      "Lightning Bolt",
      "Island",
      "Mystery",
    ]);
  });
  test("counts each chosen value as a filter, and a range once", () => {
    expect(
      activeFilterCount({
        ...NEUTRAL_QUERY,
        sets: ["M11"],
        colors: ["R"],
        types: ["Instant"],
        rarities: ["common"],
        manaMin: 0,
        manaMax: 3,
      }),
    ).toBe(5);
  });
  test("printings list most copies first", () => {
    expect(bolt.printings.map((p) => p.copies)).toEqual([4, 2, 1]);
  });
});
