import { describe, expect, test } from "bun:test";
import type {
  CatalogueLabels,
  DeckDetail,
} from "@spellwake/application-contracts";
import {
  entriesFrom,
  linesFrom,
  printingLabel,
  refusalMessage,
  sameEntries,
  settingsOf,
} from "../decks/connected-mapping";
import { addCopy, exportDeckText, moveAll } from "./deck-draft";

const id = (n: number): string =>
  `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const [BOLT, KRENKO, TOKEN, BOLT_PRINT, DECK, REVISION, FORMAT] = [
  1, 2, 3, 4, 5, 6, 7,
].map(id) as [string, string, string, string, string, string, string];
const detail: DeckDetail = {
  deck: {
    id: DECK,
    name: "Goblins",
    mode: "collection",
    currentRevisionId: REVISION,
    format: { id: FORMAT, key: "commander", name: "Commander" },
    appearance: {
      backId: id(8),
      sleeveId: id(9),
      diceId: id(10),
      playmatId: null,
    },
  },
  revision: {
    id: REVISION,
    number: "3",
    entries: [
      {
        definitionId: BOLT,
        printingId: BOLT_PRINT,
        section: "main",
        quantity: "4",
      },
      {
        definitionId: KRENKO,
        printingId: null,
        section: "commander",
        quantity: "1",
      },
      {
        definitionId: TOKEN,
        printingId: null,
        section: "tokens",
        quantity: "2",
      },
    ],
  },
};
const labels: CatalogueLabels = {
  cards: [
    { id: BOLT, name: "Lightning Bolt", oracleRef: "bolt" },
    { id: KRENKO, name: "Krenko, Mob Boss", oracleRef: "krenko" },
  ],
  printings: [
    {
      id: BOLT_PRINT,
      definitionId: BOLT,
      externalPrintingRef: "msc-806",
      rarity: "uncommon",
      finish: "nonfoil",
      collectorNumber: "806",
      language: "en",
      set: { id: id(11), name: "Secret Lair", code: "msc" },
    },
  ],
};

describe("account decks in the builder", () => {
  test("a saved deck becomes named rows and saves back unchanged", () => {
    const { lines, kept } = linesFrom(detail, labels);
    expect(
      lines.map(({ name, section, quantity }) => [name, section, quantity]),
    ).toEqual([
      ["Lightning Bolt", "main", "4"],
      ["Krenko, Mob Boss", "commander", "1"],
    ]);
    // A section the builder does not show is carried, not dropped.
    expect(kept).toEqual([
      {
        definitionId: TOKEN,
        printingId: null,
        section: "tokens",
        quantity: "2",
      },
    ]);
    const back = entriesFrom(lines, kept);
    expect(back.status).toBe("success");
    if (back.status === "success")
      expect(sameEntries(back.entries, detail.revision?.entries ?? [])).toBe(
        true,
      );
  });

  test("edits save the exact card and printing that were shown", () => {
    const { lines, kept } = linesFrom(detail, labels);
    const moved = moveAll(lines, lines[0]?.id ?? "", "sideboard");
    const added = addCopy(moved, "Krenko, Mob Boss", "main", () => "new", {
      definitionId: KRENKO,
      printingId: null,
    });
    const result = entriesFrom(added, kept);
    expect(result.status).toBe("success");
    if (result.status !== "success") return;
    expect(result.entries).toContainEqual({
      definitionId: BOLT,
      printingId: BOLT_PRINT,
      section: "sideboard",
      quantity: "4",
    });
    expect(result.entries).toContainEqual({
      definitionId: KRENKO,
      printingId: null,
      section: "main",
      quantity: "1",
    });
    expect(sameEntries(result.entries, detail.revision?.entries ?? [])).toBe(
      false,
    );
  });

  test("a row known by name alone is never sent as a card", () => {
    const { lines, kept } = linesFrom(detail, labels);
    const typed = addCopy(lines, "Mystery Card", "main", () => "new");
    expect(entriesFrom(typed, kept)).toEqual({
      status: "fail",
      name: "Mystery Card",
    });
  });

  test("a card the catalogue cannot name still shows and saves as itself", () => {
    const { lines, kept } = linesFrom(detail, { cards: [], printings: [] });
    expect(lines[0]?.name).toBe("Unknown card 00000000");
    expect(lines[0]?.card).toEqual({
      definitionId: BOLT,
      printingId: BOLT_PRINT,
    });
    expect(entriesFrom(lines, kept).status).toBe("success");
  });

  test("export names the printing where the catalogue does", () => {
    const { lines } = linesFrom(detail, labels);
    expect(exportDeckText(lines, (line) => printingLabel(line, labels))).toBe(
      "Commander\n1 Krenko, Mob Boss\n\nDeck\n4 Lightning Bolt (MSC) 806",
    );
  });

  test("settings carry everything the server compares", () => {
    expect(settingsOf(detail.deck)).toEqual({
      name: "Goblins",
      mode: "collection",
      formatId: FORMAT,
      appearance: detail.deck.appearance,
    });
  });

  test("a refusal reads as one plain sentence", () => {
    expect(
      refusalMessage({ revision: "deck changed; reload before saving" }),
    ).toBe("This deck changed elsewhere. Reopen it before saving.");
    expect(refusalMessage({ entries: "invalid deck entries" })).toBe(
      "Some cards couldn’t be saved.",
    );
    expect(refusalMessage({ deck: "deck unavailable" })).toBe(
      "This deck is unavailable.",
    );
  });
});
