/**
 * The owned card collection, as the Cards screen reads it from either the
 * account or the local preview. A source says only what it knows: anything it
 * cannot read is null, and a filter or sort that needs it is not offered.
 */

export type CardColor = "W" | "U" | "B" | "R" | "G";
export const CARD_COLORS: readonly CardColor[] = ["W", "U", "B", "R", "G"];
export const COLOR_NAMES: Record<CardColor, string> = {
  W: "White",
  U: "Blue",
  B: "Black",
  R: "Red",
  G: "Green",
};
export type Rarity = "common" | "uncommon" | "rare" | "mythic";
export const RARITIES: readonly Rarity[] = [
  "common",
  "uncommon",
  "rare",
  "mythic",
];
export const RARITY_NAMES: Record<Rarity, string> = {
  common: "Common",
  uncommon: "Uncommon",
  rare: "Rare",
  mythic: "Mythic",
};
/** The card types the filter offers, in the approved order. */
export const CARD_TYPES = [
  "Instant",
  "Creature",
  "Artifact",
  "Enchantment",
  "Land",
  "Planeswalker",
  "Sorcery",
  "Battle",
] as const;

/** One owned printing of a card: where it is from, its finish and how many copies. */
export type OwnedPrinting = {
  id: string;
  /** The set's code, such as "M11", or null when the catalogue has none. */
  setCode: string | null;
  setName: string;
  /** Collector number, or null when unknown. */
  number: string | null;
  /** Shown as written, such as "Nonfoil" or "Foil". */
  finish: string;
  rarity: Rarity | null;
  copies: number;
  /** The printed face, or null where no artwork is available. */
  faceUrl: string | null;
  /** The other face of a two-faced printing. */
  backUrl: string | null;
};

/** One card of the collection, with every printing owned of it. */
export type OwnedCard = {
  /** The card's identity. Two cards with one name stay apart. */
  id: string;
  name: string;
  /** Copies owned across every printing. */
  copies: number;
  /** Most copies first. */
  printings: readonly OwnedPrinting[];
  /** The type line, or null when unknown. */
  type: string | null;
  /** Rules text, used by search only. Printed text stays part of the card image. */
  rules: string | null;
  manaValue: number | null;
  colors: readonly CardColor[] | null;
  identity: readonly CardColor[] | null;
  /** Legality by lower-case format name, or null when unknown. */
  legality: Readonly<Record<string, string>> | null;
};

/** Which filters and sorts a source can honour. */
export type CardFacets = {
  /** Format names to filter by, or null when legality is unknown. */
  formats: readonly string[] | null;
  color: boolean;
  type: boolean;
  rarity: boolean;
  manaValue: boolean;
};

/** Why a source has no collection to give. Loading shows the neutral loading state. */
export type CardsGap =
  "loading" | "signed-out" | "unregistered" | "unavailable";
export type CardsSource =
  | { status: CardsGap }
  | {
      status: "ready";
      /** What has been read so far. More may still be arriving. */
      cards: readonly OwnedCard[];
      facets: CardFacets;
      /** True while further pages are being read. */
      isAppending: boolean;
      /** A read that failed after its retries, said once as a toast. */
      failure: string | null;
      /** True when the first read failed, so the collection is not known to be empty. */
      isUnread: boolean;
    };

export type SortKey =
  | "name-asc"
  | "name-desc"
  | "mana-asc"
  | "mana-desc"
  | "set-asc"
  | "set-desc"
  | "owned-desc"
  | "owned-asc";
export const SORTS: readonly { key: SortKey; label: string }[] = [
  { key: "name-asc", label: "Name A–Z" },
  { key: "name-desc", label: "Name Z–A" },
  { key: "mana-asc", label: "Mana value low–high" },
  { key: "mana-desc", label: "Mana value high–low" },
  { key: "set-asc", label: "Set A–Z" },
  { key: "set-desc", label: "Set Z–A" },
  { key: "owned-desc", label: "Owned most–least" },
  { key: "owned-asc", label: "Owned least–most" },
];

/** Everything that narrows or orders the gallery. Blank values restrict nothing. */
export type CardQuery = {
  search: string;
  format: string | null;
  sets: readonly string[];
  colorMode: "color" | "identity";
  colors: readonly CardColor[];
  colorless: boolean;
  types: readonly string[];
  rarities: readonly Rarity[];
  manaMin: number | null;
  manaMax: number | null;
  sort: SortKey;
};
export const NEUTRAL_QUERY: CardQuery = {
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
  sort: "name-asc",
};

/** A gallery entry: the card and the owned printing that represents it here. */
export type CardMatch = { card: OwnedCard; printing: OwnedPrinting };

/** The key a set is filtered by: its code where it has one, else its name. */
export const setKey = (printing: OwnedPrinting): string =>
  printing.setCode ?? printing.setName;
/** "M11 · 146", or as much of it as is known. */
export const printingLabel = (printing: OwnedPrinting): string =>
  [printing.setCode ?? printing.setName, printing.number]
    .filter(Boolean)
    .join(" · ");
/** A catalogue finish as written for people: "nonfoil" is "Nonfoil", "foil" is "Foil". */
export const finishLabel = (finish: string): string =>
  finish.charAt(0).toUpperCase() + finish.slice(1);
export const copiesLabel = (copies: number): string =>
  `${copies} ${copies === 1 ? "copy" : "copies"}`;

/** How many separate restrictions apply, counted the way the Filters button shows it. */
export function activeFilterCount(query: CardQuery): number {
  return (
    (query.format ? 1 : 0) +
    query.sets.length +
    query.colors.length +
    (query.colorless ? 1 : 0) +
    query.types.length +
    query.rarities.length +
    (query.manaMin !== null || query.manaMax !== null ? 1 : 0)
  );
}

const typeMatches = (card: OwnedCard, types: readonly string[]): boolean =>
  types.length === 0 ||
  (card.type !== null &&
    types.some((type) =>
      card.type
        ?.toLowerCase()
        .split(/[\s—-]+/)
        .includes(type.toLowerCase()),
    ));

function colorMatches(card: OwnedCard, query: CardQuery): boolean {
  if (query.colors.length === 0 && !query.colorless) return true;
  const colors = query.colorMode === "color" ? card.colors : card.identity;
  if (colors === null) return false;
  return (
    (query.colorless && colors.length === 0) ||
    query.colors.some((color) => colors.includes(color))
  );
}

function manaMatches(card: OwnedCard, query: CardQuery): boolean {
  if (query.manaMin === null && query.manaMax === null) return true;
  if (card.manaValue === null) return false;
  return (
    (query.manaMin === null || card.manaValue >= query.manaMin) &&
    (query.manaMax === null || card.manaValue <= query.manaMax)
  );
}

/**
 * The cards that match, each with the owned printing that represents it.
 * Facets combine with AND and the choices within one facet with OR. Set and
 * rarity must hold for the same owned printing, which then represents the
 * card; otherwise the printing with the most copies does.
 */
export function matchCards(
  cards: readonly OwnedCard[],
  query: CardQuery,
): CardMatch[] {
  const needle = query.search.trim().toLowerCase();
  const matches = cards.flatMap((card): CardMatch[] => {
    if (
      needle &&
      ![card.name, card.type ?? "", card.rules ?? ""].some((text) =>
        text.toLowerCase().includes(needle),
      )
    )
      return [];
    if (query.format) {
      const record = card.legality?.[query.format.toLowerCase()];
      if (record !== "legal" && record !== "restricted") return [];
    }
    if (
      !colorMatches(card, query) ||
      !typeMatches(card, query.types) ||
      !manaMatches(card, query)
    )
      return [];
    const printing = card.printings.find(
      (candidate) =>
        (query.sets.length === 0 || query.sets.includes(setKey(candidate))) &&
        (query.rarities.length === 0 ||
          (candidate.rarity !== null &&
            query.rarities.includes(candidate.rarity))),
    );
    return printing ? [{ card, printing }] : [];
  });
  return sortMatches(matches, query.sort);
}

const byName = (a: CardMatch, b: CardMatch): number =>
  a.card.name.localeCompare(b.card.name) || a.card.id.localeCompare(b.card.id);

function sortMatches(matches: CardMatch[], sort: SortKey): CardMatch[] {
  const direction = sort.endsWith("-desc") ? -1 : 1;
  const compare = (a: CardMatch, b: CardMatch): number => {
    switch (sort) {
      case "name-asc":
      case "name-desc":
        return direction * byName(a, b);
      case "mana-asc":
      case "mana-desc": {
        // Unknown mana values sort last either way.
        const x = a.card.manaValue;
        const y = b.card.manaValue;
        if (x === null || y === null)
          return x === y ? byName(a, b) : x === null ? 1 : -1;
        return direction * (x - y) || byName(a, b);
      }
      case "set-asc":
      case "set-desc":
        return (
          direction * a.printing.setName.localeCompare(b.printing.setName) ||
          byName(a, b)
        );
      case "owned-desc":
      case "owned-asc":
        return direction * (a.card.copies - b.card.copies) || byName(a, b);
    }
  };
  return [...matches].sort(compare);
}

/** The owned sets, each once, by name, for the set filter. */
export function ownedSets(
  cards: readonly OwnedCard[],
): { key: string; name: string; code: string | null }[] {
  const sets = new Map<
    string,
    { key: string; name: string; code: string | null }
  >();
  for (const card of cards)
    for (const printing of card.printings)
      sets.set(setKey(printing), {
        key: setKey(printing),
        name: printing.setName,
        code: printing.setCode,
      });
  return [...sets.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/** Printings in the order the inspector lists them: most copies first. */
export function orderPrintings(
  printings: readonly OwnedPrinting[],
): OwnedPrinting[] {
  return [...printings].sort(
    (a, b) =>
      b.copies - a.copies ||
      printingLabel(a).localeCompare(printingLabel(b)) ||
      a.finish.localeCompare(b.finish),
  );
}
