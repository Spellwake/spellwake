/** Shared presentation identities used by the demo library and desktop review. */
export const DEMO_FORMATS = ["Standard", "Modern", "Commander"] as const;
export const demoDeckPresets = [
  { name: "Goblins", format: "Standard", colors: ["R"] },
  { name: "Blue Control", format: "Modern", colors: ["U"] },
  { name: "Elves", format: "Commander", colors: ["G"] },
  { name: "Angels", format: "Standard", colors: ["W"] },
  { name: "Graveyard", format: "Modern", colors: ["B"] },
  { name: "Dragons", format: "Commander", colors: ["R"] },
] as const;

export function demoDeckIdentity(index: number, numbered = false) {
  const preset = demoDeckPresets[index % demoDeckPresets.length]!;
  const number = index + 1;
  const group = numbered ? "9000" : "8000";
  return {
    ...preset,
    id: `00000000-0000-4000-${group}-${String(number).padStart(12, "0")}`,
    name: numbered ? `${preset.name} ${number}` : preset.name,
  };
}
