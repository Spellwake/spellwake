import type {
  CardColor,
  CardFacets,
  CardQuery,
  Rarity,
} from "../collection/card-sources";

/**
 * Market Listings, as the screen reads them from the local preview or the
 * account: whole lots of one card printing or one pack product, each bought
 * whole at its total price. A source says only what it knows; a filter or
 * sort it cannot honour over every listing is not offered.
 */

/** One printing and finish of a card, with what the filters need when it is known. */
export type ListingCard = {
  kind: "card";
  /** The card's identity, for search and the filters. */
  cardId: string;
  name: string;
  setCode: string | null;
  setName: string;
  number: string | null;
  /** "Nonfoil" or "Foil", as written for people. */
  finish: string;
  rarity: Rarity | null;
  faceUrl: string | null;
  artUrl: string | null;
  type: string | null;
  rules: string | null;
  manaValue: number | null;
  colors: readonly CardColor[] | null;
  identity: readonly CardColor[] | null;
  legality: Readonly<Record<string, string>> | null;
};
export type ListingPack = {
  kind: "pack";
  /** "Foundations Play Booster". */
  product: string;
  setName: string;
  setCode: string | null;
};
export type ListingItem = ListingCard | ListingPack;

export type Listing = {
  id: string;
  /** The revision a purchase must name, as the service writes it. */
  revision: string;
  item: ListingItem;
  quantity: number;
  /** The total for the whole lot, or null when it is too large to show exactly. */
  total: number | null;
  /** The total as the service writes it, which a purchase must name. */
  totalText: string;
  seller: { id: string; name: string };
  /** Your own active listing: it can be cancelled, never bought. */
  isMine: boolean;
  /** Higher is newer, or null where the source does not know. */
  listedAt: number | null;
};

/** Something you own that a listing can offer: one printing and finish, or one pack product. */
export type ListableAsset = {
  id: string;
  item: ListingItem;
  /** Units that can be listed now: owned, less what trades and listings hold. Only this is shown. */
  available: number;
};

export type ListingSort = "newest" | "price-asc" | "price-desc" | "name-asc";
export const SORT_LABELS: Record<ListingSort, string> = {
  newest: "Newest",
  "price-asc": "Price: Low to high",
  "price-desc": "Price: High to low",
  "name-asc": "Name: A – Z",
};
export type ListingKind = "all" | "cards" | "packs";
export const KIND_LABELS: Record<ListingKind, string> = {
  all: "All items",
  cards: "Cards",
  packs: "Packs",
};
export type Finish = "any" | "nonfoil" | "foil";
export const FINISH_LABELS: Record<Finish, string> = {
  any: "Any",
  nonfoil: "Nonfoil",
  foil: "Foil",
};

/** The card filters of Collection, plus Finish and the total price. */
export type ListingQuery = Omit<CardQuery, "sort"> & {
  kind: ListingKind;
  mine: boolean;
  finish: Finish;
  priceMin: number | null;
  priceMax: number | null;
  sort: ListingSort;
};
export const NEUTRAL_LISTING_QUERY: ListingQuery = {
  search: "",
  format: null,
  sets: [],
  colorMode: "color",
  colors: [],
  colorless: false,
  types: [],
  rarities: [],
  manaMin: null,
  manaMax: null,
  kind: "all",
  mine: false,
  finish: "any",
  priceMin: null,
  priceMax: null,
  sort: "newest",
};

export type ListingFacets = CardFacets & {
  /** The sorts this source can honour over every listing, default first. */
  sorts: readonly ListingSort[];
};

/** A command the screen keeps open until its outcome is known. */
export type OperationStage =
  | { kind: "pending" }
  /** Sent, and no answer came. Only Check status may settle it. */
  | { kind: "unconfirmed"; isChecking: boolean }
  /** The service said no; nothing changed. The dialog can close or try again. */
  | { kind: "rejected"; message: string };
export type Operation =
  | { kind: "buy"; listing: Listing; stage: OperationStage }
  | {
      kind: "create";
      asset: ListableAsset;
      quantity: number;
      total: number;
      stage: OperationStage;
    }
  | { kind: "cancel"; listing: Listing; stage: OperationStage };

/** What an accepted command leads to, said once as a toast. */
export type Completed =
  | { kind: "bought"; listing: Listing }
  | { kind: "created"; listingId: string | null }
  | { kind: "cancelled"; listingId: string };

export type ListingsGap =
  "loading" | "signed-out" | "unregistered" | "unavailable";
export type ListingsSource =
  | { status: ListingsGap }
  | {
      status: "ready";
      listings: readonly Listing[];
      facets: ListingFacets;
      isAppending: boolean;
      /** True when the first read failed, so the market is not known to be empty. */
      isUnread: boolean;
      /** A read that failed after its retries, said once. */
      failure: string | null;
      dismissFailure: () => void;
      /** Coins you can spend, or null while unknown. */
      balance: number | null;
      /** What you can list, or null while it is being read. */
      assets: readonly ListableAsset[] | null;
      /** False where the account may not create, buy or cancel. */
      canTrade: boolean;
      operation: Operation | null;
      completed: Completed | null;
      dismissCompleted: () => void;
      buy: (listingId: string) => void;
      create: (assetId: string, quantity: number, total: number) => void;
      cancel: (listingId: string) => void;
      /** Asks again about an unconfirmed command, never sending a new one. */
      checkStatus: () => void;
      /** Clears a rejected command, so its dialog can close. */
      dismissOperation: () => void;
    };
export type ReadyListings = Extract<ListingsSource, { status: "ready" }>;

export const listingName = (item: ListingItem): string =>
  item.kind === "card" ? item.name : item.product;
export const quantityLabel = (item: ListingItem, quantity: number): string =>
  item.kind === "pack"
    ? `${quantity} ${quantity === 1 ? "pack" : "packs"}`
    : `${quantity} ${quantity === 1 ? "copy" : "copies"}`;
/** The set key the set filter uses: the code where there is one, else the name. */
export const listingSetKey = (item: ListingItem): string =>
  item.setCode ?? item.setName;

/** "Purchased 4 copies of Lightning Bolt." */
export const purchasedMessage = (listing: Listing): string =>
  `Purchased ${quantityLabel(listing.item, listing.quantity)} of ${listingName(listing.item)}.`;

function cardMatches(card: ListingCard, query: ListingQuery): boolean {
  if (query.format) {
    const record = card.legality?.[query.format.toLowerCase()];
    if (record !== "legal" && record !== "restricted") return false;
  }
  if (query.colors.length > 0 || query.colorless) {
    const colors = query.colorMode === "color" ? card.colors : card.identity;
    if (colors === null) return false;
    if (!(
      (query.colorless && colors.length === 0) ||
      query.colors.some((color) => colors.includes(color))
    ))
      return false;
  }
  if (
    query.types.length > 0 &&
    !(
      card.type !== null &&
      query.types.some((type) =>
        card.type
          ?.toLowerCase()
          .split(/[\s—-]+/)
          .includes(type.toLowerCase()),
      )
    )
  )
    return false;
  if (
    query.rarities.length > 0 &&
    !(card.rarity !== null && query.rarities.includes(card.rarity))
  )
    return false;
  if (query.manaMin !== null || query.manaMax !== null) {
    if (card.manaValue === null) return false;
    if (query.manaMin !== null && card.manaValue < query.manaMin) return false;
    if (query.manaMax !== null && card.manaValue > query.manaMax) return false;
  }
  if (query.finish !== "any" && card.finish.toLowerCase() !== query.finish)
    return false;
  return true;
}

/**
 * The listings that match, in the chosen order. Search matches card and
 * product names, set names and set codes. Facets combine with AND and the
 * choices within one facet with OR. Card-only facets apply only while Cards
 * is chosen, so they never hide packs. Only my listings shows your own
 * active listings; otherwise your own are listed too, and can only be
 * cancelled.
 */
export function matchListings(
  listings: readonly Listing[],
  query: ListingQuery,
): Listing[] {
  const needle = query.search.trim().toLowerCase();
  const matched = listings.filter((listing) => {
    const { item } = listing;
    if (query.mine && !listing.isMine) return false;
    if (query.kind === "cards" && item.kind !== "card") return false;
    if (query.kind === "packs" && item.kind !== "pack") return false;
    if (
      needle &&
      ![listingName(item), item.setName, item.setCode ?? ""].some((text) =>
        text.toLowerCase().includes(needle),
      )
    )
      return false;
    if (query.sets.length > 0 && !query.sets.includes(listingSetKey(item)))
      return false;
    if (query.priceMin !== null || query.priceMax !== null) {
      if (listing.total === null) return false;
      if (query.priceMin !== null && listing.total < query.priceMin)
        return false;
      if (query.priceMax !== null && listing.total > query.priceMax)
        return false;
    }
    if (query.kind === "cards" && item.kind === "card")
      return cardMatches(item, query);
    return true;
  });
  return sortListings(matched, query.sort);
}

/** A stable order: the chosen key, then name, then identity. */
export function sortListings(
  listings: readonly Listing[],
  sort: ListingSort,
): Listing[] {
  const byName = (a: Listing, b: Listing) =>
    listingName(a.item).localeCompare(listingName(b.item)) ||
    a.id.localeCompare(b.id);
  const price = (listing: Listing) => listing.total ?? Number.POSITIVE_INFINITY;
  return [...listings].sort((a, b) => {
    switch (sort) {
      case "newest":
        return (b.listedAt ?? 0) - (a.listedAt ?? 0) || byName(a, b);
      case "price-asc":
        return price(a) - price(b) || byName(a, b);
      case "price-desc":
        return price(b) - price(a) || byName(a, b);
      case "name-asc":
        return byName(a, b);
    }
  });
}

/** How many restrictions apply for the chosen kind, as the Filters button counts them. */
export function listingFilterCount(query: ListingQuery): number {
  const common =
    query.sets.length +
    (query.priceMin !== null || query.priceMax !== null ? 1 : 0);
  if (query.kind !== "cards") return common;
  return (
    common +
    (query.format ? 1 : 0) +
    query.colors.length +
    (query.colorless ? 1 : 0) +
    query.types.length +
    query.rarities.length +
    (query.manaMin !== null || query.manaMax !== null ? 1 : 0) +
    (query.finish !== "any" ? 1 : 0)
  );
}

/** The sets the listings come from, each once, by name, for the set filter. */
export function listingSets(
  listings: readonly Listing[],
  kind: ListingKind,
): { key: string; name: string; code: string | null }[] {
  const sets = new Map<
    string,
    { key: string; name: string; code: string | null }
  >();
  for (const { item } of listings) {
    if (kind === "cards" && item.kind !== "card") continue;
    if (kind === "packs" && item.kind !== "pack") continue;
    sets.set(listingSetKey(item), {
      key: listingSetKey(item),
      name: item.setName,
      code: item.setCode,
    });
  }
  return [...sets.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * The listing to select after one leaves the list: the next, or the
 * previous at the end, or none.
 */
export function nextSelection(
  shown: readonly Listing[],
  removedId: string,
): string | null {
  const index = shown.findIndex((listing) => listing.id === removedId);
  if (index < 0) return shown[0]?.id ?? null;
  return shown[index + 1]?.id ?? shown[index - 1]?.id ?? null;
}

/** A whole number typed into a field, blank as null, anything else invalid. */
export function readWhole(text: string): number | null | "invalid" {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  return /^\d{1,15}$/.test(trimmed) ? Number(trimmed) : "invalid";
}

/** What is wrong with a listing's terms, field by field, or nulls when they can be sent. */
export function termsProblems(
  quantity: string,
  total: string,
  available: number,
): { quantity: string | null; total: string | null } {
  const count = readWhole(quantity);
  const price = readWhole(total);
  return {
    quantity:
      count === "invalid" || count === null || count < 1
        ? "Enter a whole number above 0."
        : count > available
          ? `Only ${available} available.`
          : null,
    total:
      price === "invalid" || price === null || price < 1
        ? "Enter a whole number above 0."
        : null,
  };
}
