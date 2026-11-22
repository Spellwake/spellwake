/**
 * Copied from the deck-box asset study (`assets/deck-box/appearance.ts`) so the package
 * holds all of its own code and type-checks without the private asset tree.
 * The study keeps its own copy for its previews. A change to one belongs in
 * the other.
 */
/** Deck color identity controls material treatment independently from cover art. */
export type ManaColor = "W" | "U" | "B" | "R" | "G";
export type MaterialTone = ManaColor | "gold" | "colorless";

export const MANA_ORDER: readonly ManaColor[] = ["W", "U", "B", "R", "G"];
export const MANA_INK: Record<ManaColor | "C", string> = {
  W: "#f6e4a5",
  U: "#559deb",
  B: "#bc8ae1",
  R: "#ef4533",
  G: "#77b86b",
  C: "#c4c6c3",
};
const MATERIAL: Record<Exclude<MaterialTone, "R">, [number, number, number]> = {
  W: [0.12, 0.19, 0.96],
  U: [0.585, 0.79, 0.83],
  B: [0.74, 0.18, 0.25],
  G: [0.35, 0.66, 0.58],
  gold: [0.115, 0.66, 0.88],
  colorless: [0.6, 0.055, 0.7],
};

function rgbToHsv(r: number, g: number, b: number): [number, number, number] {
  const max = Math.max(r, g, b),
    min = Math.min(r, g, b),
    delta = max - min;
  let h = 0;
  if (delta) {
    if (max === r) h = ((g - b) / delta + 6) % 6;
    else if (max === g) h = (b - r) / delta + 2;
    else h = (r - g) / delta + 4;
  }
  return [h / 6, max ? delta / max : 0, max];
}

function hsvToHex(h: number, s: number, v: number): string {
  const sector = h * 6,
    part = Math.floor(sector),
    fraction = sector - part;
  const p = v * (1 - s),
    q = v * (1 - fraction * s),
    t = v * (1 - (1 - fraction) * s);
  const channels = [
    [v, t, p],
    [q, v, p],
    [p, v, t],
    [p, q, v],
    [t, p, v],
    [v, p, q],
  ][((part % 6) + 6) % 6];
  if (!channels) return "#000000";
  return `#${channels
    .map((value) =>
      Math.round(Math.min(1, Math.max(0, value)) * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

/** Repaint one extracted material tone while retaining its relative shade and texture. */
export function tintMaterial(source: string, target: MaterialTone): string {
  if (target === "R") return source;
  const r = parseInt(source.slice(1, 3), 16) / 255;
  const g = parseInt(source.slice(3, 5), 16) / 255;
  const b = parseInt(source.slice(5, 7), 16) / 255;
  const [h, s, v] = rgbToHsv(r, g, b);
  const [targetH, targetS, targetV] = MATERIAL[target];
  const highlight = Math.min(
    1,
    Math.max(0, (v - 0.7) / 0.3) * Math.max(0, (0.9 - s) / 0.55),
  );
  const mappedV = Math.min(1, (v * targetV) / 0.84);
  return hsvToHex(
    (targetH + (h < 0.5 ? h : h - 1) * 0.22 + 1) % 1,
    Math.min(1, (s * targetS) / 0.88),
    mappedV * (1 - highlight) + v * highlight,
  );
}

/** Zero colors is colorless, two split the box, and three or more use gold. */
export function materialForIdentity(
  colors: readonly ManaColor[],
): [MaterialTone, MaterialTone] {
  const first = colors[0];
  if (!first) return ["colorless", "colorless"];
  if (colors.length > 2) return ["gold", "gold"];
  return [first, colors[1] ?? first];
}
