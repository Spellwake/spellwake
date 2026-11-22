// Owned quantities and printing IDs follow the approved desktop review fixture.
export const sets: Record<string, string> = {
  MH2: "Modern Horizons 2",
  M21: "Core Set 2021",
  M11: "Magic 2011",
  "2X2": "Double Masters 2022",
  M19: "Core Set 2019",
  CMM: "Commander Masters",
};
export const colors = ["White", "Blue", "Black", "Red", "Green"] as const;
export const formats = [
  "Standard",
  "Modern",
  "Pioneer",
  "Legacy",
  "Vintage",
  "Commander",
];
export const rarities = ["Common", "Uncommon", "Rare", "Mythic"];
export const cardTypes = [
  "Artifact",
  "Battle",
  "Creature",
  "Enchantment",
  "Instant",
  "Land",
  "Planeswalker",
  "Sorcery",
];
const modern = ["Modern", "Legacy", "Vintage", "Commander"];
type Printing = {
  set: string;
  number: string;
  rarity: string;
  count: number;
  foil: boolean;
};
function print(
  set: string,
  number: string,
  rarity: string,
  count: number,
  foil = false,
): Printing {
  return { set, number, rarity, count, foil };
}
type Card = {
  name: string;
  type: string;
  rules: string;
  mana: number;
  color: string;
  identity: string;
  formats: string[];
  printings: Printing[];
};
export const inventory: Card[] = [
  {
    name: "Counterspell",
    type: "Instant",
    rules: "Counter target spell.",
    mana: 2,
    color: "Blue",
    identity: "Blue",
    formats: modern,
    printings: [print("MH2", "267", "Uncommon", 2)],
  },
  {
    name: "Island",
    type: "Basic Land — Island",
    rules: "({T}: Add {U}.)",
    mana: 0,
    color: "Colorless",
    identity: "Blue",
    formats,
    printings: [print("M21", "264", "Common", 20)],
  },
  {
    name: "Lightning Bolt",
    type: "Instant",
    rules: "Lightning Bolt deals 3 damage to any target.",
    mana: 1,
    color: "Red",
    identity: "Red",
    formats: modern,
    printings: [
      print("M11", "146", "Common", 4),
      print("2X2", "117", "Uncommon", 2),
      print("2X2", "117", "Uncommon", 1, true),
    ],
  },
  {
    name: "Llanowar Elves",
    type: "Creature — Elf Druid",
    rules: "{T}: Add {G}.",
    mana: 1,
    color: "Green",
    identity: "Green",
    formats,
    printings: [print("M19", "314", "Common", 4)],
  },
  {
    name: "Mountain",
    type: "Basic Land — Mountain",
    rules: "({T}: Add {R}.)",
    mana: 0,
    color: "Colorless",
    identity: "Red",
    formats,
    printings: [print("M11", "243", "Common", 20)],
  },
  {
    name: "Opt",
    type: "Instant",
    rules: "Scry 1. Draw a card.",
    mana: 1,
    color: "Blue",
    identity: "Blue",
    formats,
    printings: [print("M21", "059", "Common", 6)],
  },
  {
    name: "Sol Ring",
    type: "Artifact",
    rules: "{T}: Add {C}{C}.",
    mana: 1,
    color: "Colorless",
    identity: "Colorless",
    formats: ["Vintage", "Commander"],
    printings: [print("CMM", "410", "Uncommon", 1)],
  },
  {
    name: "Swords to Plowshares",
    type: "Instant",
    rules:
      "Exile target creature. Its controller gains life equal to its power.",
    mana: 1,
    color: "White",
    identity: "White",
    formats: ["Legacy", "Vintage", "Commander"],
    printings: [print("CMM", "387", "Uncommon", 2)],
  },
];
export const cardNames = inventory.map((card) => card.name);
export const copies = inventory.map((card) =>
  card.printings.reduce((total, p) => total + p.count, 0),
);
export const printLabel = (p: Printing) => p.set + " · " + p.number;
export type Facet = "sets" | "colors" | "types" | "rarities";
export type Bounds = { min: string; max: string };
export type Filters = Record<Facet, string[]> &
  Bounds & { format: string; identity: boolean };
export const emptyFilters: Filters = {
  format: "",
  sets: [],
  colors: [],
  types: [],
  rarities: [],
  min: "",
  max: "",
  identity: false,
};
export const boltFilters: Filters = {
  ...emptyFilters,
  sets: ["M11"],
  colors: ["Red"],
  types: ["Instant"],
  rarities: ["Common"],
  min: "0",
  max: "3",
};
export const sortOrders = [
  "Name A–Z",
  "Name Z–A",
  "Mana value low–high",
  "Mana value high–low",
  "Set A–Z",
  "Set Z–A",
  "Most copies",
  "Fewest copies",
] as const;
export function rangeError({ min, max }: Bounds) {
  if (
    ![min, max].every((v) => /^\d*$/.test(v) && Number.isSafeInteger(Number(v)))
  )
    return "Enter a nonnegative whole number.";
  return min && max && Number(min) > Number(max)
    ? "Maximum must be at least minimum."
    : "";
}
export function toggleFacet(f: Filters, facet: Facet, value: string): Filters {
  return {
    ...f,
    [facet]: f[facet].includes(value)
      ? f[facet].filter((v) => v !== value)
      : [...f[facet], value],
  };
}
export function activeChips(f: Filters) {
  const chips: { label: string; remove: () => Filters }[] = [];
  if (f.format)
    chips.push({ label: f.format, remove: () => ({ ...f, format: "" }) });
  for (const facet of ["sets", "colors", "types", "rarities"] as const) {
    for (const value of f[facet])
      chips.push({ label: value, remove: () => toggleFacet(f, facet, value) });
  }
  if (f.min || f.max)
    chips.push({
      label:
        "MV " +
        (f.min && f.max
          ? f.min + "–" + f.max
          : f.min
            ? "≥" + f.min
            : "≤" + f.max),
      remove: () => ({ ...f, min: "", max: "" }),
    });
  return chips;
}
export function printingMatches(p: Printing, f: Filters) {
  return (
    (!f.sets.length || f.sets.includes(p.set)) &&
    (!f.rarities.length || f.rarities.includes(p.rarity))
  );
}
export function browseCards(query: string, f: Filters, sort: string) {
  const search = query.trim().toLowerCase();
  const entries = inventory.flatMap((card, index) => {
    if (
      ![card.name, card.type, card.rules].some((text) =>
        text.toLowerCase().includes(search),
      ) ||
      (f.format && !card.formats.includes(f.format)) ||
      (f.types.length &&
        !f.types.some((type) => card.type.split(/\W+/).includes(type))) ||
      (f.colors.length &&
        !f.colors.includes(f.identity ? card.identity : card.color)) ||
      (f.min && card.mana < Number(f.min)) ||
      (f.max && card.mana > Number(f.max))
    )
      return [];
    const printing = card.printings.findIndex((p) => printingMatches(p, f));
    return printing < 0 ? [] : [{ card, index, printing }];
  });
  return entries.sort((a, b) => {
    const name = a.card.name.localeCompare(b.card.name);
    const setName = (entry: typeof a) =>
      sets[entry.card.printings[entry.printing]!.set]!;
    switch (sort) {
      case "Name Z–A":
        return -name;
      case "Mana value low–high":
        return a.card.mana - b.card.mana || name;
      case "Mana value high–low":
        return b.card.mana - a.card.mana || name;
      case "Set A–Z":
        return setName(a).localeCompare(setName(b)) || name;
      case "Set Z–A":
        return setName(b).localeCompare(setName(a)) || name;
      case "Most copies":
        return copies[b.index]! - copies[a.index]! || name;
      case "Fewest copies":
        return copies[a.index]! - copies[b.index]! || name;
      default:
        return name;
    }
  });
}
