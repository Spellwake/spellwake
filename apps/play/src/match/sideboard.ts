/** One card of a deck between games: how many are in the main deck and how many in the sideboard. */
export type SideboardCard = {
  /** The card's identity in the deck: its definition and printing. */
  key: string;
  name: string;
  main: number;
  side: number;
};

type Entry = {
  definitionId: string;
  printingId: string | null;
  section: string;
  quantity: string;
};

const keyOf = (entry: { definitionId: string; printingId: string | null }) =>
  `${entry.definitionId}/${entry.printingId ?? ""}`;

/**
 * A deck's main deck and sideboard as one list, a row per card, by name.
 * Cards in other sections (the builder's Considering) are not part of a game
 * and are left where they are.
 */
export function sideboardCards(
  entries: readonly Entry[],
  nameOf: (definitionId: string) => string | undefined,
): SideboardCard[] {
  const cards = new Map<string, SideboardCard>();
  for (const entry of entries) {
    if (entry.section !== "main" && entry.section !== "sideboard") continue;
    const key = keyOf(entry);
    const card = cards.get(key) ?? {
      key,
      name: nameOf(entry.definitionId) ?? "Card",
      main: 0,
      side: 0,
    };
    if (entry.section === "main") card.main += Number(entry.quantity);
    else card.side += Number(entry.quantity);
    cards.set(key, card);
  }
  return [...cards.values()].sort(
    (a, b) => a.name.localeCompare(b.name) || a.key.localeCompare(b.key),
  );
}

/** Moves one copy of a card into the main deck or out to the sideboard. */
export function moveCard(
  cards: readonly SideboardCard[],
  key: string,
  to: "main" | "side",
): SideboardCard[] {
  return cards.map((card) => {
    if (card.key !== key) return card;
    if (to === "main" && card.side > 0)
      return { ...card, main: card.main + 1, side: card.side - 1 };
    if (to === "side" && card.main > 0)
      return { ...card, main: card.main - 1, side: card.side + 1 };
    return card;
  });
}

export function counts(cards: readonly SideboardCard[]): {
  main: number;
  side: number;
} {
  return cards.reduce(
    (total, card) => ({
      main: total.main + card.main,
      side: total.side + card.side,
    }),
    { main: 0, side: 0 },
  );
}

/** Whether any card has moved between the main deck and the sideboard. */
export function changed(
  start: readonly SideboardCard[],
  now: readonly SideboardCard[],
): boolean {
  const before = new Map(start.map((card) => [card.key, card.main]));
  return now.some((card) => before.get(card.key) !== card.main);
}

/**
 * The deck's entries with the main deck and sideboard as they now stand.
 * Every other section is kept exactly as it was.
 */
export function withSideboard<E extends Entry>(
  entries: readonly E[],
  cards: readonly SideboardCard[],
): Entry[] {
  const kept: Entry[] = entries.filter(
    (entry) => entry.section !== "main" && entry.section !== "sideboard",
  );
  const identity = new Map(
    entries.map((entry) => [
      keyOf(entry),
      { definitionId: entry.definitionId, printingId: entry.printingId },
    ]),
  );
  for (const card of cards) {
    const id = identity.get(card.key);
    if (!id) continue;
    if (card.main > 0)
      kept.push({ ...id, section: "main", quantity: String(card.main) });
    if (card.side > 0)
      kept.push({ ...id, section: "sideboard", quantity: String(card.side) });
  }
  return kept;
}

/**
 * Whether two lists hold exactly the same cards, wherever each copy sits.
 * Between games the server takes only a list that does.
 */
export function samePool(a: readonly Entry[], b: readonly Entry[]): boolean {
  const total = (entries: readonly Entry[]) => {
    const sums = new Map<string, number>();
    for (const entry of entries)
      sums.set(
        keyOf(entry),
        (sums.get(keyOf(entry)) ?? 0) + Number(entry.quantity),
      );
    return sums;
  };
  const left = total(a);
  const right = total(b);
  return (
    left.size === right.size &&
    [...left].every(([key, count]) => right.get(key) === count)
  );
}
