import type {
  CatalogueLabels,
  CustomDeckEntry,
  DeckDetail,
  DeckSettings,
  DeckSummary,
  SaveDeck,
} from "@spellwake/application-contracts";
import { mergeDeckEntries } from "../data/deck-resolution";
import { DECK_TEXT_SECTIONS } from "../data/deck-text";
import type { DeckLine, DeckSection } from "../data/deck-draft";

/**
 * Translation between the account's saved decks and the rows the builder
 * edits. Nothing here talks to the server, so every rule is covered by tests.
 */
type Entry = SaveDeck["entries"][number];

const isShown = (section: string): section is DeckSection =>
  DECK_TEXT_SECTIONS.some((known) => known === section);

/**
 * The saved deck as builder rows. A row's ID is the saved entry's own
 * identity, so it is stable across reloads. Entries in a section the builder
 * does not show are returned in `kept` and saved back unchanged.
 */
export function linesFrom(
  detail: DeckDetail,
  labels: CatalogueLabels,
  customNames: ReadonlyMap<string, string> = new Map(),
): { lines: DeckLine[]; kept: Entry[]; keptCustom: CustomDeckEntry[] } {
  const names = new Map(labels.cards.map((card) => [card.id, card.name]));
  const lines: DeckLine[] = [];
  const kept: Entry[] = [];
  const keptCustom: CustomDeckEntry[] = [];
  for (const entry of detail.revision?.entries ?? []) {
    if (!isShown(entry.section)) {
      kept.push(entry);
      continue;
    }
    lines.push({
      id: `${entry.definitionId}/${entry.printingId ?? ""}/${entry.section}`,
      // A card the catalogue cannot name still shows, and still saves, as itself.
      name:
        names.get(entry.definitionId) ??
        `Unknown card ${entry.definitionId.slice(0, 8)}`,
      quantity: entry.quantity,
      section: entry.section,
      card: { definitionId: entry.definitionId, printingId: entry.printingId },
    });
  }
  for (const custom of detail.revision?.customEntries ?? []) {
    if (!isShown(custom.section)) {
      keptCustom.push(custom);
      continue;
    }
    lines.push({
      id: `custom/${custom.customCardRevisionId}/${custom.section}`,
      name:
        customNames.get(custom.customCardRevisionId) ??
        `Custom card ${custom.customCardRevisionId.slice(0, 8)}`,
      quantity: custom.quantity,
      section: custom.section,
      customCardRevisionId: custom.customCardRevisionId,
    });
  }
  return { lines, kept, keptCustom };
}

/**
 * Builder rows as entries to save. Rows for the same card, printing and
 * section are combined. Fails when a row has no card identity, because a name
 * alone is never sent as a card.
 */
export function entriesFrom(
  lines: readonly DeckLine[],
  kept: readonly Entry[],
  keptCustom: readonly CustomDeckEntry[] = [],
):
  | {
      status: "success";
      entries: Entry[];
      customEntries: CustomDeckEntry[];
    }
  | { status: "fail"; name: string } {
  const unmatched = lines.find(
    (line) => !line.card && !line.customCardRevisionId,
  );
  if (unmatched) return { status: "fail", name: unmatched.name };
  const merged = mergeDeckEntries([
    ...lines.flatMap((line) =>
      line.card
        ? [
            {
              definitionId: line.card.definitionId,
              printingId: line.card.printingId,
              section: line.section,
              quantity: line.quantity,
            },
          ]
        : [],
    ),
    ...kept,
  ]);
  if (merged.status !== "success") {
    return { status: "fail", name: lines[0]?.name ?? "" };
  }

  const customMap = new Map<string, { customCardRevisionId: string; section: string; quantity: bigint }>();
  for (const line of lines) {
    if (!line.customCardRevisionId) continue;
    const key = `${line.customCardRevisionId}/${line.section}`;
    const prev = customMap.get(key);
    const qty = BigInt(line.quantity);
    if (prev) prev.quantity += qty;
    else
      customMap.set(key, {
        customCardRevisionId: line.customCardRevisionId,
        section: line.section,
        quantity: qty,
      });
  }
  for (const keptItem of keptCustom) {
    const key = `${keptItem.customCardRevisionId}/${keptItem.section}`;
    const prev = customMap.get(key);
    const qty = BigInt(keptItem.quantity);
    if (prev) prev.quantity += qty;
    else
      customMap.set(key, {
        customCardRevisionId: keptItem.customCardRevisionId,
        section: keptItem.section,
        quantity: qty,
      });
  }

  return {
    status: "success",
    entries: merged.entries,
    customEntries: [...customMap.values()].map((item) => ({
      customCardRevisionId: item.customCardRevisionId,
      section: item.section,
      quantity: String(item.quantity),
    })),
  };
}

/** Whether two custom entry lists hold the same revisions and quantities. */
export function sameCustomEntries(
  a: readonly CustomDeckEntry[],
  b: readonly CustomDeckEntry[],
): boolean {
  const key = (entries: readonly CustomDeckEntry[]): string =>
    entries
      .map(
        (entry) =>
          `${entry.section}/${entry.customCardRevisionId}/${entry.quantity}`,
      )
      .sort()
      .join("\n");
  return key(a) === key(b);
}

/** Whether two entry lists hold the same cards, whatever their order. */
export function sameEntries(a: readonly Entry[], b: readonly Entry[]): boolean {
  const key = (entries: readonly Entry[]): string =>
    entries
      .map(
        (entry) =>
          `${entry.section}/${entry.definitionId}/${entry.printingId ?? ""}/${entry.quantity}`,
      )
      .sort()
      .join("\n");
  return key(a) === key(b);
}

/** The settings a change is compared against: everything the server holds beside the cards. */
export const settingsOf = (deck: DeckSummary): DeckSettings => ({
  name: deck.name,
  mode: deck.mode,
  formatId: deck.format.id,
  appearance: deck.appearance,
});

/** The set and collector number of a row's printing, where the catalogue names both. */
export function printingLabel(
  line: DeckLine,
  labels: CatalogueLabels,
): { set: string; number: string } | null {
  const printing = labels.printings.find(
    (candidate) => candidate.id === line.card?.printingId,
  );
  return printing?.set.code && printing.collectorNumber
    ? { set: printing.set.code.toUpperCase(), number: printing.collectorNumber }
    : null;
}

/** A refused command as one short sentence for the player. */
export function refusalMessage(errors: Record<string, string>): string {
  if ("revision" in errors || "settings" in errors)
    return "This deck changed elsewhere. Reopen it before saving.";
  if ("entries" in errors) return "Some cards couldn’t be saved.";
  if ("cosmetics" in errors) return "A chosen cosmetic is unavailable.";
  if ("account" in errors) return "Your account is unavailable.";
  return "This deck is unavailable.";
}
