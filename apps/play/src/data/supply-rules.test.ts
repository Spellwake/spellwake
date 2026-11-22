import { describe, expect, test } from "bun:test";
import { previewTerms } from "../market/demo-supply";
import {
  ballotDates,
  boughtMessage,
  matchTiles,
  quantityProblem,
  shortDate,
  tileAvailability,
  type QuoteTerms,
  type SupplyTile,
} from "../market/supply-sources";

const terms = (remaining: number, total: number | null): QuoteTerms => ({
  quantity: 1,
  remaining,
  total,
  policyId: "p",
  revision: "1",
  totalText: total === null ? null : String(total),
});
const product = (
  id: string,
  setName: string,
  code: string,
  quote: QuoteTerms | null,
  quoteState: "checking" | "ready" | "failed" = "ready",
): SupplyTile => ({
  kind: "product",
  id,
  setId: `set-${code}`,
  setName,
  setCode: code,
  product: "Play Booster",
  fullName: `${setName} Play Booster`,
  quote,
  quoteState,
});
const tiles: SupplyTile[] = [
  product("fdn", "Foundations", "FDN", terms(420, 1000)),
  product("dsk", "Duskmourn", "DSK", terms(0, null)),
  product("mh3", "Modern Horizons 3", "MH3", null, "checking"),
  {
    kind: "set",
    id: "p15a",
    setName: "15th Anniversary Cards",
    setCode: "P15A",
  },
];
const ids = (list: readonly SupplyTile[]) => list.map((tile) => tile.id);

describe("the supply gallery", () => {
  test("an unknown quote is checking, never sold out or unavailable", () => {
    expect(tiles.map(tileAvailability)).toEqual([
      "available",
      "sold-out",
      "checking",
      "unavailable",
    ]);
    expect(ids(matchTiles(tiles, "", "sold-out"))).toEqual(["dsk"]);
    expect(ids(matchTiles(tiles, "", "unavailable"))).toEqual(["p15a"]);
  });

  test("search matches set names, codes and product names", () => {
    expect(ids(matchTiles(tiles, "p15a", "all"))).toEqual(["p15a"]);
    expect(ids(matchTiles(tiles, "horizons", "all"))).toEqual(["mh3"]);
    expect(ids(matchTiles(tiles, "play booster", "available"))).toEqual([
      "fdn",
    ]);
  });
});

describe("buying packs", () => {
  test("a quantity is a whole number from 1 to 1000", () => {
    expect(quantityProblem("1")).toBeNull();
    expect(quantityProblem("1000")).toBeNull();
    for (const text of ["0", "1001", "1.5", "", "-2"])
      expect(quantityProblem(text)).toBe(
        "Enter a whole number from 1 to 1000.",
      );
  });

  test("a total is summed pack by pack, never quantity times the first price", () => {
    const item = {
      id: "x",
      setCode: "FDN",
      setName: "Foundations",
      product: "Play Booster",
      stock: 420,
      base: 1000,
      step: 3,
      packId: "p",
    };
    expect(previewTerms(item, 0, 1).total).toBe(1000);
    expect(previewTerms(item, 0, 2).total).toBe(2003);
    // Another buyer took one: the same two packs now cost more.
    expect(previewTerms(item, 1, 2)).toMatchObject({
      remaining: 419,
      total: 2009,
    });
    expect(previewTerms(item, 419, 2)).toMatchObject({
      remaining: 1,
      total: null,
    });
  });

  test("the toast names the packs bought", () => {
    expect(boughtMessage("Foundations", 1)).toBe("Bought 1 Foundations pack.");
    expect(boughtMessage("Bloomburrow", 3)).toBe("Bought 3 Bloomburrow packs.");
  });
});

describe("the vote's dates", () => {
  test("a vote closes 168 hours before the next UTC quarter", () => {
    const { closes, nextDrop } = ballotDates(new Date("2026-11-22T12:00:00Z"));
    expect(shortDate(nextDrop)).toBe("Jan 1");
    expect(shortDate(closes)).toBe("Dec 25");
    expect(nextDrop.getUTCFullYear()).toBe(2027);
    expect(
      shortDate(ballotDates(new Date("2027-02-10T00:00:00Z")).closes),
    ).toBe("Mar 25");
  });
});
