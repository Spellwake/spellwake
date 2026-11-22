import { describe, expect, test } from "bun:test";
import {
  listingFilterCount,
  matchListings,
  NEUTRAL_LISTING_QUERY,
  nextSelection,
  sortListings,
  termsProblems,
  type Listing,
  type ListingCard,
  type ListingQuery,
} from "../market/listing-sources";

const card = (name: string, extra: Partial<ListingCard> = {}): ListingCard => ({
  kind: "card",
  cardId: name,
  name,
  setCode: "M11",
  setName: "Magic 2011",
  number: null,
  finish: "Nonfoil",
  rarity: "common",
  faceUrl: null,
  artUrl: null,
  type: "Instant",
  rules: null,
  manaValue: 1,
  colors: ["R"],
  identity: ["R"],
  legality: { modern: "legal" },
  ...extra,
});
const listing = (
  id: string,
  item: Listing["item"],
  total: number,
  extra: Partial<Listing> = {},
): Listing => ({
  id,
  revision: "1",
  item,
  quantity: 1,
  total,
  totalText: String(total),
  seller: { id: "mira", name: "Mira" },
  isMine: false,
  listedAt: null,
  ...extra,
});
const pack = {
  kind: "pack",
  product: "Foundations Play Booster",
  setName: "Foundations",
  setCode: "FDN",
} as const;
const market = [
  listing("a", card("Lightning Bolt"), 500, { listedAt: 3 }),
  listing(
    "b",
    card("Counterspell", { colors: ["U"], identity: ["U"], finish: "Foil" }),
    350,
    { listedAt: 2 },
  ),
  listing("c", pack, 6000, { listedAt: 1 }),
  listing("d", card("Sol Ring", { colors: [], identity: [] }), 700, {
    isMine: true,
    listedAt: 4,
  }),
];
const query = (patch: Partial<ListingQuery>): ListingQuery => ({
  ...NEUTRAL_LISTING_QUERY,
  ...patch,
});
const ids = (listings: readonly Listing[]) => listings.map((l) => l.id);

describe("matching listings", () => {
  test("card-only filters never hide packs while All items or Packs is chosen", () => {
    const red = query({ colors: ["R"], finish: "foil", manaMin: 3 });
    expect(ids(matchListings(market, { ...red, kind: "all" }))).toContain("c");
    expect(ids(matchListings(market, { ...red, kind: "packs" }))).toEqual([
      "c",
    ]);
    expect(ids(matchListings(market, { ...red, kind: "cards" }))).toEqual([]);
  });

  test("Cards applies colour, finish and colourless as Collection does", () => {
    expect(
      ids(matchListings(market, query({ kind: "cards", colors: ["U"] }))),
    ).toEqual(["b"]);
    expect(
      ids(matchListings(market, query({ kind: "cards", finish: "foil" }))),
    ).toEqual(["b"]);
    expect(
      ids(matchListings(market, query({ kind: "cards", colorless: true }))),
    ).toEqual(["d"]);
  });

  test("search matches names, set names and set codes; Only my listings shows your own", () => {
    expect(ids(matchListings(market, query({ search: "fdn" })))).toEqual(["c"]);
    expect(
      ids(
        matchListings(
          market,
          query({ search: "magic 2011", sort: "name-asc" }),
        ),
      ),
    ).toEqual(["b", "a", "d"]);
    expect(ids(matchListings(market, query({ mine: true })))).toEqual(["d"]);
  });

  test("the price range is inclusive and applies to every kind", () => {
    expect(
      ids(
        matchListings(
          market,
          query({ priceMin: 350, priceMax: 500, sort: "price-asc" }),
        ),
      ),
    ).toEqual(["b", "a"]);
  });
});

describe("ordering and selection", () => {
  test("sorts break ties by name, then identity, so the order is stable", () => {
    const tied = [
      listing("z", card("Opt"), 100),
      listing("y", card("Opt"), 100),
      listing("x", card("Abrade"), 100),
    ];
    expect(ids(sortListings(tied, "price-asc"))).toEqual(["x", "y", "z"]);
    expect(ids(sortListings(market, "newest"))).toEqual(["d", "a", "b", "c"]);
    expect(ids(sortListings(market, "price-desc"))).toEqual([
      "c",
      "d",
      "a",
      "b",
    ]);
  });

  test("a removed listing hands selection to the next, then the previous, then none", () => {
    const shown = market.slice(0, 3);
    expect(nextSelection(shown, "a")).toBe("b");
    expect(nextSelection(shown, "c")).toBe("b");
    expect(nextSelection([market[0]!], "a")).toBeNull();
  });

  test("the Filters count includes card facets only while Cards is chosen", () => {
    const filters = query({ sets: ["M11"], colors: ["R"], priceMax: 600 });
    expect(listingFilterCount(filters)).toBe(2);
    expect(listingFilterCount({ ...filters, kind: "cards" })).toBe(3);
  });
});

describe("listing terms", () => {
  test("quantity is a whole number from one to what is available; the total a whole number above zero", () => {
    expect(termsProblems("4", "500", 4)).toEqual({
      quantity: null,
      total: null,
    });
    expect(termsProblems("5", "0", 4)).toEqual({
      quantity: "Only 4 available.",
      total: "Enter a whole number above 0.",
    });
    expect(termsProblems("1.5", "", 4).quantity).toBe(
      "Enter a whole number above 0.",
    );
  });
});
