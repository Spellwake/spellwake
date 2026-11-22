import { describe, expect, test } from "bun:test";
import {
  addCopy,
  assignRole,
  clearRole,
  type DeckLine,
  exportDeckText,
  groupSection,
  moveAll,
  roleFor,
  sectionCount,
  setCopies,
} from "./deck-draft";
import { readDeckIntake } from "./deck-text";

const row = (
  id: string,
  name: string,
  quantity: string,
  section: DeckLine["section"] = "main",
): DeckLine => ({ id, name, quantity, section });
const facts: Record<string, { type: string; manaValue: number }> = {
  "Goblin Matron": { type: "Creature — Goblin", manaValue: 3 },
  "Skirk Prospector": { type: "Creature — Goblin", manaValue: 1 },
  Shock: { type: "Instant", manaValue: 1 },
  Mountain: { type: "Basic Land — Mountain", manaValue: 0 },
};
const lookup = (name: string) => facts[name];

describe("deck draft", () => {
  test("adding merges into the card's row in that section only", () => {
    const start = [row("a", "Shock", "3"), row("b", "Shock", "1", "sideboard")];
    const next = addCopy(start, "shock", "main", () => "new");
    expect(next).toHaveLength(2);
    expect(next[0]?.quantity).toBe("4");
    expect(next[1]?.quantity).toBe("1");
    expect(
      addCopy(start, "Mountain", "considering", () => "new"),
    ).toContainEqual(row("new", "Mountain", "1", "considering"));
  });

  test("zero copies removes the row and large counts stay exact", () => {
    const start = [row("a", "Shock", "1")];
    expect(setCopies(start, "a", 0n)).toEqual([]);
    const huge = 10n ** 30n;
    expect(setCopies(start, "a", huge)[0]?.quantity).toBe(String(huge));
  });

  test("moving every copy merges with the card already in the destination", () => {
    const start = [row("a", "Shock", "3"), row("b", "Shock", "2", "sideboard")];
    const merged = moveAll(start, "a", "sideboard");
    expect(merged).toEqual([row("b", "Shock", "5", "sideboard")]);
    expect(moveAll(start, "a", "considering")[0]).toEqual(
      row("a", "Shock", "3", "considering"),
    );
    expect(sectionCount(merged, "main")).toBe(0n);
    expect(sectionCount(merged, "sideboard")).toBe(5n);
  });

  test("groups follow type order and sort inside each group", () => {
    const entries = [
      row("1", "Mountain", "20"),
      row("2", "Shock", "4"),
      row("3", "Goblin Matron", "4"),
      row("4", "Skirk Prospector", "4"),
      row("5", "Unlisted Card", "1"),
      row("6", "Shock", "2", "sideboard"),
    ];
    const byType = groupSection(
      entries,
      "main",
      { groupBy: "type", sortBy: "name" },
      lookup,
    );
    expect(byType.map((group) => group.label)).toEqual([
      "Creatures",
      "Instants",
      "Lands",
      "Other",
    ]);
    expect(byType[0]?.count).toBe(8n);
    expect(byType[0]?.entries.map((entry) => entry.name)).toEqual([
      "Goblin Matron",
      "Skirk Prospector",
    ]);
    const byMana = groupSection(
      entries,
      "main",
      { groupBy: "type", sortBy: "mana" },
      lookup,
    );
    expect(byMana[0]?.entries[0]?.name).toBe("Skirk Prospector");
    expect(
      groupSection(
        entries,
        "main",
        { groupBy: "mana", sortBy: "name" },
        lookup,
      ).map((group) => group.label),
    ).toEqual([
      "Mana value 0",
      "Mana value 1",
      "Mana value 3",
      "Unknown mana value",
    ]);
  });

  test("a role takes one copy and returns it to Main when cleared", () => {
    const start = [row("a", "Krenko, Mob Boss", "2")];
    const assigned = assignRole(
      start,
      { entryId: "a" },
      "commander",
      () => "role",
    );
    expect(assigned).toEqual([
      row("a", "Krenko, Mob Boss", "1"),
      row("role", "Krenko, Mob Boss", "1", "commander"),
    ]);
    expect(clearRole(assigned, "role", () => "new")).toEqual([
      row("a", "Krenko, Mob Boss", "2"),
    ]);
    const last = assignRole(
      [row("a", "Krenko, Mob Boss", "1")],
      { entryId: "a" },
      "commander",
      () => "role",
    );
    expect(last).toEqual([row("role", "Krenko, Mob Boss", "1", "commander")]);
    expect(
      assignRole([], { name: "Krenko, Mob Boss" }, "commander", () => "role"),
    ).toEqual([row("role", "Krenko, Mob Boss", "1", "commander")]);
  });

  test("export keeps sections and quantities, with headings only when needed", () => {
    expect(
      exportDeckText([row("a", "Shock", "4"), row("b", "Mountain", "20")]),
    ).toBe("4 Shock\n20 Mountain");
    expect(
      exportDeckText([
        row("a", "Shock", "4"),
        row("b", "Abrade", "2", "sideboard"),
        row("c", "Krenko, Mob Boss", "1", "commander"),
      ]),
    ).toBe(
      "Commander\n1 Krenko, Mob Boss\n\nDeck\n4 Shock\n\nSideboard\n2 Abrade",
    );
  });

  test("export adds a printing hint only where the printing is known", () => {
    const printings: Record<string, { set: string; number: string }> = {
      "Lightning Bolt": { set: "MSC", number: "806" },
    };
    expect(
      exportDeckText(
        [row("a", "Lightning Bolt", "4"), row("b", "Mountain", "20")],
        ({ name }) => printings[name],
      ),
    ).toBe("4 Lightning Bolt (MSC) 806\n20 Mountain");
  });

  test("roles come from the printed card and the format", () => {
    const krenko = { type: "Legendary Creature — Goblin Warrior", rules: "" };
    const jegantha = {
      type: "Legendary Creature — Elemental Elk",
      rules:
        "Companion — No card in your starting deck has more than one of the same mana symbol in its mana cost.",
    };
    expect(roleFor(krenko, "Commander")).toBe("commander");
    expect(roleFor(krenko, "Modern")).toBeNull();
    expect(roleFor(jegantha, "Modern")).toBe("companion");
    expect(
      roleFor({ type: "Legendary Artifact", rules: "" }, "Commander"),
    ).toBeNull();
    expect(
      roleFor({ type: "Creature — Goblin", rules: "" }, "Commander"),
    ).toBeNull();
    expect(roleFor(undefined, "Commander")).toBeNull();
  });

  test("an exported list reads back through the importer unchanged", () => {
    const printings: Record<string, { set: string; number: string }> = {
      "Krenko, Mob Boss": { set: "DDT", number: "52" },
      "Mogg War Marshal": { set: "PLST", number: "DMR-131" },
      "Krenko's Command": { set: "CLU", number: "139" },
    };
    const text = exportDeckText(
      [
        row("a", "Krenko, Mob Boss", "1", "commander"),
        row("b", "Mogg War Marshal", "4"),
        row("c", "Mountain", "20"),
        row("d", "Krenko's Command", "2", "sideboard"),
        row("e", "Goblin Guide", "1", "considering"),
      ],
      ({ name }) => printings[name],
    );
    const { lines, errors } = readDeckIntake(text);
    expect(errors).toEqual([]);
    expect(
      lines.map(({ quantity, name, section, printingHint }) => ({
        quantity,
        name,
        section,
        printingHint,
      })),
    ).toEqual([
      {
        quantity: "1",
        name: "Krenko, Mob Boss",
        section: "commander",
        printingHint: { setCode: "DDT", collectorNumber: "52" },
      },
      {
        quantity: "4",
        name: "Mogg War Marshal",
        section: "main",
        printingHint: { setCode: "PLST", collectorNumber: "DMR-131" },
      },
      { quantity: "20", name: "Mountain", section: "main", printingHint: null },
      {
        quantity: "2",
        name: "Krenko's Command",
        section: "sideboard",
        printingHint: { setCode: "CLU", collectorNumber: "139" },
      },
      {
        quantity: "1",
        name: "Goblin Guide",
        section: "considering",
        printingHint: null,
      },
    ]);
  });

  test("a card's identity follows it through every edit", () => {
    const bolt = { definitionId: "d-bolt", printingId: "p-bolt" };
    const krenko = { definitionId: "d-krenko", printingId: null };
    const start: DeckLine[] = [
      { ...row("a", "Lightning Bolt", "2"), card: bolt },
    ];
    const added = addCopy(start, "Krenko, Mob Boss", "main", () => "b", krenko);
    expect(added[1]?.card).toEqual(krenko);
    const moved = moveAll(added, "a", "sideboard");
    expect(moved[0]).toMatchObject({ section: "sideboard", card: bolt });
    const led = assignRole(moved, { entryId: "b" }, "commander", () => "c");
    expect(led.find((line) => line.section === "commander")?.card).toEqual(
      krenko,
    );
    const back = clearRole(led, "c", () => "d");
    expect(back.find((line) => line.name === "Krenko, Mob Boss")).toMatchObject(
      {
        section: "main",
        card: krenko,
      },
    );
    expect(
      assignRole(
        [],
        { name: "Krenko, Mob Boss", card: krenko },
        "commander",
        () => "e",
      )[0]?.card,
    ).toEqual(krenko);
  });
});
