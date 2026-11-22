import type { CardImage, PrintedMode } from "@spellwake/ui";

const images = import.meta.glob<string>(
  "../../../../../.impeccable/review/open-play/approved-screens/implementation/full-match-assets/*.webp",
  { eager: true, query: "?inline", import: "default" },
);
const byName = Object.fromEntries(
  Object.entries(images).map(([path, src]) => [
    path.split("/").pop()!.replace(".webp", ""),
    src,
  ]),
);
export const seats = ["You", "Alex", "Morgan", "Sam", "Riley", "Jordan"];
export const portrait = (index: number) => byName["seat" + index]!;
const names = {
  elf: "Llanowar Elves",
  bear: "Grizzly Bears",
  angel: "Serra Angel",
  krenko: "Krenko, Mob Boss",
  chandra: "Chandra Nalaar",
  ball: "Crystal Ball",
  pacifism: "Pacifism",
  thopter: "Ornithopter",
  mountain: "Mountain",
  island: "Island",
  forest: "Forest",
  plains: "Plains",
  bolt: "Lightning Bolt",
  counter: "Counterspell",
  growth: "Giant Growth",
  cryptic: "Cryptic Command",
  bond: "Common Bond",
  abrade: "Abrade",
  arc: "Arc Lightning",
  blaze: "Blaze",
};
export type Card = keyof typeof names;
export const cardImage = (id: Card): CardImage => ({
  name: names[id],
  src: byName[id]!,
  width: 488,
  height: 680,
});
export const cardName = (id: Card) => names[id];
export const sourceFor = (family: string): Card =>
  (
    ({
      BC: "bolt",
      MC: "abrade",
      MM: "cryptic",
      MX: "cryptic",
      MT: "bond",
      DC: "arc",
      X: "blaze",
    }) as Record<string, Card>
  )[family] ?? "bolt";
const printed = (
  id: string,
  label: string,
  y: number,
  bottom: number,
): PrintedMode => ({
  id,
  label,
  region: { x: 37, y, width: 414, height: bottom - y },
});
export const abradeModes = [
  printed("damage", "Abrade deals 3 damage to target creature.", 449, 497),
  printed("destroy", "Destroy target artifact.", 497, 526),
];
export const crypticModes = [
  printed("counter", "Counter target spell.", 459, 487),
  printed("return", "Return target permanent to its owner’s hand.", 487, 540),
  printed("tap", "Tap all creatures your opponents control.", 540, 593),
  printed("draw", "Draw a card.", 593, 622),
];
export const bondModes = [
  printed("first", "Put a +1/+1 counter on target creature.", 436, 490),
  printed("second", "Put a +1/+1 counter on target creature.", 491, 545),
];
