import deckPrintings from "../../../../../output/imagegen/guildhall-screen-set/components/assets/catalog.json";
import searchPrintings from "../../../../../output/imagegen/guildhall-screen-set/components/assets/search-cards/catalog.json";
import { inventory } from "./card-inventory";

const assetRoot =
  "../../../../../output/imagegen/guildhall-screen-set/components/";
const images = import.meta.glob<string>(
  "../../../../../output/imagegen/guildhall-screen-set/components/assets/{*,search-cards/*}.jpg",
  { eager: true, import: "default", query: "?inline" },
);

type BuilderCard = {
  name: string;
  type: string;
  manaValue: number;
  cost?: string;
  face?: string;
  art?: string;
  inventoryIndex?: number;
};

function lookupName(name: string) {
  return name.trim().replaceAll("’", "'").toLowerCase();
}

// Persisted demo entries are names. Resolve against the known review printings
// without changing the spelling, quantity or section in the saved deck.
const cards = new Map<string, BuilderCard>();
inventory.forEach((card, inventoryIndex) => {
  cards.set(lookupName(card.name), {
    name: card.name,
    type: card.type,
    manaValue: card.mana,
    inventoryIndex,
  });
});
for (const p of [...deckPrintings, ...searchPrintings]) {
  cards.set(lookupName(p.name), {
    name: p.name,
    type: p.type,
    manaValue: p.mana_value,
    cost: p.mana,
    face: images[assetRoot + p.image]!,
    art: images[assetRoot + p.art_image]!,
  });
}
export const builderCard = (name: string) => cards.get(lookupName(name));
export { searchPrintings };
const typeGroups = [
  ["Creature", "Creatures"],
  ["Artifact", "Artifacts"],
  ["Enchantment", "Enchantments"],
  ["Instant", "Instants"],
  ["Sorcery", "Sorceries"],
  ["Planeswalker", "Planeswalkers"],
  ["Battle", "Battles"],
  ["Land", "Lands"],
] as const;
const compareName = new Intl.Collator("en", { sensitivity: "base" });
export function deckGroups(
  holdings: Record<string, bigint>,
  byMana: boolean,
  sortMana: boolean,
  lookup: (
    name: string,
  ) => { type: string; manaValue: number | null } | undefined = builderCard,
) {
  const groups = new Map<
    string,
    { label: string; order: number; rows: string[]; count: bigint }
  >();
  for (const [name, count] of Object.entries(holdings)) {
    if (count <= 0) continue;
    const card = lookup(name);
    const cardTypes = card?.type.split(" — ")[0]?.split(" ");
    const typeIndex = typeGroups.findIndex(([type]) =>
      cardTypes?.includes(type),
    );
    const title = byMana
      ? String(card?.manaValue ?? "Unknown mana value")
      : (typeGroups[typeIndex]?.[1] ?? "Other");
    const order = byMana
      ? card?.manaValue
      : typeIndex >= 0
        ? typeIndex
        : undefined;
    const group = groups.get(title) ?? {
      label: title,
      order: order ?? Infinity,
      rows: [],
      count: 0n,
    };
    group.rows.push(name);
    group.count += count;
    groups.set(title, group);
  }
  for (const group of groups.values()) {
    group.rows.sort((a, b) => {
      const manaOrder = sortMana
        ? (lookup(a)?.manaValue ?? Infinity) -
          (lookup(b)?.manaValue ?? Infinity)
        : 0;
      return manaOrder || compareName.compare(a, b) || a.localeCompare(b);
    });
  }
  return [...groups.values()].sort((a, b) => a.order - b.order);
}
