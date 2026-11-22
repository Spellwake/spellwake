import type {
  CatalogueLabels,
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
): { lines: DeckLine[]; kept: Entry[] } {
  const names = new Map(labels.cards.map((card) => [card.id, card.name]));
  const lines: DeckLine[] = [];
  const kept: Entry[] = [];
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
  return { lines, kept };
}

/**
 * Builder rows as entries to save. Rows for the same card, printing and
 * section are combined. Fails when a row has no card identity, because a name
 * alone is never sent as a card.
 */
export function entriesFrom(
  lines: readonly DeckLine[],
  kept: readonly Entry[],
): { status: "success"; entries: Entry[] } | { status: "fail"; name: string } {
  const unmatched = lines.find((line) => !line.card);
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
  return merged.status === "success"
    ? merged
    : { status: "fail", name: lines[0]?.name ?? "" };
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
