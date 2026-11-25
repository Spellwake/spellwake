import { DECK_TEXT_SECTIONS } from "./deck-text";

export type DeckSection = (typeof DECK_TEXT_SECTIONS)[number];
/** A source's own identity for a card. Preview lists, which know cards by name alone, have none. */
export type CardRef = { definitionId: string; printingId: string | null };
/**
 * One row of a deck being edited. The builder works on names and quantities;
 * `card` rides along untouched so a source can save the exact card it supplied.
 */
export type DeckLine = {
  id: string;
  name: string;
  quantity: string;
  section: DeckSection;
  card?: CardRef;
  /** Open Play custom card revision identity (mutually exclusive with `card`). */
  customCardRevisionId?: string;
};
const row = (
  id: string,
  from: {
    name: string;
    card?: CardRef | undefined;
    customCardRevisionId?: string | undefined;
  },
  section: DeckSection,
): DeckLine => ({
  id,
  name: from.name,
  quantity: "1",
  section,
  ...(from.card ? { card: from.card } : {}),
  ...(from.customCardRevisionId
    ? { customCardRevisionId: from.customCardRevisionId }
    : {}),
});

/** The three ordinary sections the builder edits. Role sections are assigned separately. */
export const BUILDER_SECTIONS = ["main", "sideboard", "considering"] as const;
export type BuilderSection = (typeof BUILDER_SECTIONS)[number];

export type CardFacts = { type: string; manaValue: number | null };
export type DeckGroup = {
  label: string;
  /** Total copies in the group. Quantities are exact, so this is a bigint. */
  count: bigint;
  entries: DeckLine[];
};

const sameCard = (a: string, b: string): boolean =>
  a.trim().replaceAll("’", "'").toLowerCase() ===
  b.trim().replaceAll("’", "'").toLowerCase();

export const copiesOf = (entry: DeckLine): bigint => BigInt(entry.quantity);

/** Total copies in one section. */
export function sectionCount(
  entries: readonly DeckLine[],
  section: BuilderSection,
): bigint {
  return entries
    .filter((entry) => entry.section === section)
    .reduce((total, entry) => total + copiesOf(entry), 0n);
}

const sameLineIdentity = (
  entry: DeckLine,
  name: string,
  customCardRevisionId?: string,
): boolean => {
  if (entry.customCardRevisionId || customCardRevisionId) {
    return entry.customCardRevisionId === customCardRevisionId;
  }
  return sameCard(entry.name, name);
};

/** Adds one copy to a section, merging into the card's existing row there. */
export function addCopy(
  entries: readonly DeckLine[],
  name: string,
  section: BuilderSection,
  newId: () => string,
  card?: CardRef,
  customCardRevisionId?: string,
): DeckLine[] {
  const existing = entries.find(
    (entry) =>
      entry.section === section &&
      sameLineIdentity(entry, name, customCardRevisionId),
  );
  if (!existing)
    return [...entries, row(newId(), { name, card, customCardRevisionId }, section)];
  return entries.map((entry) =>
    entry === existing
      ? { ...entry, quantity: String(copiesOf(entry) + 1n) }
      : entry,
  );
}

/** Sets a row's copies. Zero or less removes the row. */
export function setCopies(
  entries: readonly DeckLine[],
  entryId: string,
  copies: bigint,
): DeckLine[] {
  if (copies <= 0n) return entries.filter((entry) => entry.id !== entryId);
  return entries.map((entry) =>
    entry.id === entryId ? { ...entry, quantity: String(copies) } : entry,
  );
}

/**
 * Moves every copy of a row to another section. If the card is already there,
 * the quantities merge into that row and the moved row disappears.
 */
export function moveAll(
  entries: readonly DeckLine[],
  entryId: string,
  to: BuilderSection,
): DeckLine[] {
  const moving = entries.find((entry) => entry.id === entryId);
  if (!moving || moving.section === to) return [...entries];
  const target = entries.find(
    (entry) =>
      entry.section === to &&
      sameLineIdentity(entry, moving.name, moving.customCardRevisionId),
  );
  if (!target)
    return entries.map((entry) =>
      entry === moving ? { ...entry, section: to } : entry,
    );
  return entries
    .filter((entry) => entry !== moving)
    .map((entry) =>
      entry === target
        ? { ...entry, quantity: String(copiesOf(entry) + copiesOf(moving)) }
        : entry,
    );
}

const TYPE_GROUPS = [
  ["Creature", "Creatures"],
  ["Artifact", "Artifacts"],
  ["Enchantment", "Enchantments"],
  ["Instant", "Instants"],
  ["Sorcery", "Sorceries"],
  ["Planeswalker", "Planeswalkers"],
  ["Battle", "Battles"],
  ["Land", "Lands"],
] as const;
const byName = new Intl.Collator("en", { sensitivity: "base" });

/** One section's rows, grouped by card type or mana value and sorted within each group. */
export function groupSection(
  entries: readonly DeckLine[],
  section: BuilderSection,
  options: { groupBy: "type" | "mana"; sortBy: "name" | "mana" },
  lookup: (name: string) => CardFacts | undefined,
): DeckGroup[] {
  const groups = new Map<string, DeckGroup & { order: number }>();
  for (const entry of entries) {
    if (entry.section !== section) continue;
    const facts = lookup(entry.name);
    const types = facts?.type.split(" — ")[0]?.split(" ") ?? [];
    const typeIndex = TYPE_GROUPS.findIndex(([type]) => types.includes(type));
    const label =
      options.groupBy === "mana"
        ? facts?.manaValue == null
          ? "Unknown mana value"
          : `Mana value ${facts.manaValue}`
        : (TYPE_GROUPS[typeIndex]?.[1] ?? "Other");
    const order =
      options.groupBy === "mana"
        ? (facts?.manaValue ?? Infinity)
        : typeIndex >= 0
          ? typeIndex
          : Infinity;
    const group = groups.get(label) ?? { label, order, count: 0n, entries: [] };
    group.entries.push(entry);
    group.count += copiesOf(entry);
    groups.set(label, group);
  }
  const manaOf = (entry: DeckLine): number =>
    lookup(entry.name)?.manaValue ?? Infinity;
  return [...groups.values()]
    .sort((a, b) => a.order - b.order)
    .map(({ label, count, entries: rows }) => ({
      label,
      count,
      entries: rows.sort(
        (a, b) =>
          (options.sortBy === "mana" ? manaOf(a) - manaOf(b) : 0) ||
          byName.compare(a.name, b.name),
      ),
    }));
}

/** The two single-card roles a deck may assign. */
export type DeckRole = "commander" | "companion";

/**
 * Assigns one copy to a role. From a deck row, that copy leaves its section.
 * From search, pass the card name and the copy is added directly to the role.
 * The caller checks eligibility and that the role is empty.
 */
export function assignRole(
  entries: readonly DeckLine[],
  source:
    | { entryId: string }
    | { name: string; card?: CardRef; customCardRevisionId?: string },
  role: DeckRole,
  newId: () => string,
): DeckLine[] {
  if ("name" in source) return [...entries, row(newId(), source, role)];
  const from = entries.find((entry) => entry.id === source.entryId);
  if (!from) return [...entries];
  return [
    ...setCopies(entries, from.id, copiesOf(from) - 1n),
    row(newId(), from, role),
  ];
}

/** Removes a card from its role and returns that copy to Main. */
export function clearRole(
  entries: readonly DeckLine[],
  entryId: string,
  newId: () => string,
): DeckLine[] {
  const assigned = entries.find((entry) => entry.id === entryId);
  if (!assigned) return [...entries];
  return addCopy(
    entries.filter((entry) => entry !== assigned),
    assigned.name,
    "main",
    newId,
    assigned.card,
    assigned.customCardRevisionId,
  );
}

const EXPORT_HEADINGS: readonly (readonly [DeckSection, string])[] = [
  ["commander", "Commander"],
  ["companion", "Companion"],
  ["main", "Deck"],
  ["sideboard", "Sideboard"],
  ["considering", "Considering"],
];

/**
 * The deck as a plain list for export. Sections and quantities are preserved,
 * with the set and collector number where a printing is known. A deck with
 * only main cards has no headings; any other section is named.
 */
export function exportDeckText(
  entries: readonly DeckLine[],
  printingOf: (
    line: DeckLine,
  ) => { set: string; number: string } | null | undefined = () => null,
): string {
  const sections = EXPORT_HEADINGS.map(
    ([section, heading]) =>
      [heading, entries.filter((entry) => entry.section === section)] as const,
  ).filter(([, rows]) => rows.length > 0);
  const isMainOnly = sections.length === 1 && sections[0]?.[0] === "Deck";
  return sections
    .map(([heading, rows]) =>
      [
        ...(isMainOnly ? [] : [heading]),
        ...rows.map((entry) => {
          const printing = printingOf(entry);
          // The printing hint is added only where the card's record names one.
          return printing
            ? `${entry.quantity} ${entry.name} (${printing.set}) ${printing.number}`
            : `${entry.quantity} ${entry.name}`;
        }),
      ].join("\n"),
    )
    .join("\n\n");
}

/**
 * The single-card role a card may take in a format, read from what is printed
 * on it: a companion ability, or a legendary creature in Commander. Anything
 * else, including a card with no record, has no role to offer.
 */
export function roleFor(
  card: { type: string; rules: string } | undefined,
  format: string,
): DeckRole | null {
  if (!card) return null;
  if (/^Companion\b/m.test(card.rules)) return "companion";
  const types = card.type.split(" — ")[0] ?? "";
  return format === "Commander" &&
    /\bLegendary\b/.test(types) &&
    /\bCreature\b/.test(types)
    ? "commander"
    : null;
}
