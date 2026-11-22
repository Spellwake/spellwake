import * as THREE from "three";

/**
 * The wrapper's print. Its only artwork is official: the set's own symbol,
 * stamped in gold foil. Everything else is typography (the set's name and
 * the product kind) and material: metallic foil in the set's colours,
 * embossing, crimp ridges and the crinkles of a sealed pouch. No generated
 * or invented artwork is drawn.
 *
 * Layout is authored in the 2D wrapper's own units (238 × 420), so both
 * read as the same design.
 */
export type PackPalette = { hue: string; deep: string; glow: string };
export type PackPrint = {
  setName: string;
  /** The product kind under the set's name, such as "Play Booster". Empty for none. */
  kind: string;
  palette: PackPalette;
  /** The set's official symbol as an SVG, or null where none is known. */
  symbolUrl: string | null;
  /** Typeface for the printed words. */
  serif: string;
};

/** Colour, relief and finish for one side of the wrapper. */
export type SidePrint = {
  color: THREE.CanvasTexture;
  normal: THREE.CanvasTexture;
  /** R: iridescence, G: roughness, B: metalness, as three's maps read them. */
  finish: THREE.CanvasTexture;
};

const UNITS_WIDE = 238;
const UNITS_TALL = 420;
const CRIMP = UNITS_TALL * 0.054;
const GOLD = "#f2dba6";
const CREAM = "#fbf3e4";
/** Where the set symbol is stamped, and the largest it may be. */
const EMBLEM = { x: 119, y: 166, size: 104, glow: 80 };
const BACK_EMBLEM = { x: 119, y: 186, size: 42 };
/** The back's rule and set name, under its small symbol. */
const BACK_RULE_Y = 226;
const BACK_NAME_Y = 246;

/** A repeatable random stream, so a set always crinkles the same way. */
function stream(seedText: string): () => number {
  let seed = 0x811c9dc5;
  for (let index = 0; index < seedText.length; index += 1) {
    seed ^= seedText.charCodeAt(index);
    seed = Math.imul(seed, 0x01000193) >>> 0;
  }
  return () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 2 ** 32;
  };
}

function canvas(width: number): {
  element: HTMLCanvasElement;
  context: CanvasRenderingContext2D;
  scale: number;
} {
  const element = document.createElement("canvas");
  element.width = width;
  element.height = Math.round((width * UNITS_TALL) / UNITS_WIDE);
  const context = element.getContext("2d", { willReadFrequently: false });
  if (!context) throw new Error("A 2D canvas is needed to print the pack.");
  const scale = width / UNITS_WIDE;
  context.setTransform(scale, 0, 0, scale, 0, 0);
  return { element, context, scale };
}

/** An official set symbol, ready to stamp. */
type SetSymbol = { image: HTMLImageElement; aspect: number };

/**
 * Load a set symbol SVG at a known size. Symbols are drawn from their
 * viewBox, so they are sized here rather than trusting an intrinsic size.
 */
async function loadSymbol(
  url: string,
  signal?: AbortSignal,
): Promise<SetSymbol> {
  const response = await fetch(url, signal ? { signal } : {});
  if (!response.ok) throw new Error(`Could not load ${url}.`);
  const document = new DOMParser().parseFromString(
    await response.text(),
    "image/svg+xml",
  );
  const svg = document.documentElement;
  const box = (svg.getAttribute("viewBox") ?? "").split(/[\s,]+/).map(Number);
  const width = box[2] ?? Number(svg.getAttribute("width"));
  const height = box[3] ?? Number(svg.getAttribute("height"));
  if (!(width > 0 && height > 0)) throw new Error(`${url} has no size.`);
  svg.setAttribute("width", String(width * 8));
  svg.setAttribute("height", String(height * 8));
  const blob = new Blob([new XMLSerializer().serializeToString(svg)], {
    type: "image/svg+xml",
  });
  const source = URL.createObjectURL(blob);
  try {
    const image = new Image();
    image.decoding = "async";
    image.src = source;
    await image.decode();
    return { image, aspect: width / height };
  } finally {
    URL.revokeObjectURL(source);
  }
}

/**
 * Stamp the symbol, centred at (x, y) within a `size` square, in one fill:
 * gold foil on the print, white on the relief, the gold finish on the
 * finish map.
 */
function stamp(
  c: CanvasRenderingContext2D,
  symbol: SetSymbol,
  fill: (
    context: CanvasRenderingContext2D,
    height: number,
  ) => string | CanvasGradient,
  place: { x: number; y: number; size: number },
  scale: number,
): void {
  const width = symbol.aspect >= 1 ? place.size : place.size * symbol.aspect;
  const height = symbol.aspect >= 1 ? place.size / symbol.aspect : place.size;
  const ink = document.createElement("canvas");
  ink.width = Math.max(1, Math.round(width * scale));
  ink.height = Math.max(1, Math.round(height * scale));
  const context = ink.getContext("2d");
  if (!context) return;
  context.drawImage(symbol.image, 0, 0, ink.width, ink.height);
  context.globalCompositeOperation = "source-in";
  context.fillStyle = fill(context, ink.height);
  context.fillRect(0, 0, ink.width, ink.height);
  c.drawImage(ink, place.x - width / 2, place.y - height / 2, width, height);
}

/** Gold foil, bright at the top and deepening down the symbol. */
const goldFoil = (
  context: CanvasRenderingContext2D,
  height: number,
): CanvasGradient => {
  const gradient = context.createLinearGradient(0, 0, 0, height);
  gradient.addColorStop(0, "#fbecc4");
  gradient.addColorStop(0.55, GOLD);
  gradient.addColorStop(1, "#c39a52");
  return gradient;
};
const flat = (color: string) => (): string => color;

// ─── Shared drawing ─────────────────────────────────────────────────────────

/** Fit the set's name into at most two balanced lines no wider than `limit`. */
function fitTitle(
  c: CanvasRenderingContext2D,
  text: string,
  serif: string,
  limit: number,
): { lines: string[]; size: number } {
  const words = text.toUpperCase().split(/\s+/).filter(Boolean);
  const candidates: string[][] = [[words.join(" ")]];
  for (let split = 1; split < words.length; split += 1)
    candidates.push([
      words.slice(0, split).join(" "),
      words.slice(split).join(" "),
    ]);
  let best = { lines: [words.join(" ")], size: 0 };
  for (const lines of candidates) {
    let size = 22.6;
    c.font = `${size}px ${serif}`;
    const widest = Math.max(...lines.map((line) => c.measureText(line).width));
    if (widest > limit) size *= limit / widest;
    // Two lines only when one would have to shrink noticeably.
    const score = lines.length === 1 ? size : size * 0.86;
    if (score > (best.lines.length === 1 ? best.size : best.size * 0.86))
      best = { lines, size };
  }
  return best;
}

function spaced(
  c: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  tracking: number,
): void {
  // letterSpacing is not on every canvas; lay the glyphs out by hand.
  const glyphs = [...text];
  const widths = glyphs.map((glyph) => c.measureText(glyph).width);
  const total =
    widths.reduce((sum, width) => sum + width, 0) +
    tracking * (glyphs.length - 1);
  let cursor = x - total / 2;
  c.textAlign = "left";
  glyphs.forEach((glyph, index) => {
    c.fillText(glyph, cursor, y);
    cursor += (widths[index] ?? 0) + tracking;
  });
}

type Layout = {
  title: { lines: string[]; size: number };
  kind: string;
  /** Baselines, top to bottom. */
  titleY: number[];
  ruleY: number;
  kindY: number;
};
function layoutLabel(c: CanvasRenderingContext2D, print: PackPrint): Layout {
  const title = fitTitle(c, print.setName, print.serif, UNITS_WIDE * 0.86);
  const kindY = UNITS_TALL * 0.88 - 2;
  const ruleY = print.kind ? kindY - 20 : kindY - 4;
  const lineHeight = title.size * 1.02;
  const lastTitle = ruleY - 12;
  const titleY = title.lines.map(
    (_, index) => lastTitle - (title.lines.length - 1 - index) * lineHeight,
  );
  return { title, kind: print.kind.toUpperCase(), titleY, ruleY, kindY };
}

function traceRule(c: CanvasRenderingContext2D, y: number, width = 0.74): void {
  const half = (UNITS_WIDE * width) / 2;
  const gradient = c.createLinearGradient(119 - half, 0, 119 + half, 0);
  gradient.addColorStop(0, "#0000");
  gradient.addColorStop(0.5, c.fillStyle as string);
  gradient.addColorStop(1, "#0000");
  const previous = c.fillStyle;
  c.fillStyle = gradient;
  c.fillRect(119 - half, y - 0.5, half * 2, 1);
  c.fillStyle = previous;
  c.save();
  c.translate(119, y);
  c.rotate(Math.PI / 4);
  c.fillRect(-3, -3, 6, 6);
  c.restore();
}

/** Pinked edges at both crimps, cut through the alpha. */
function cutSerration(c: CanvasRenderingContext2D): void {
  const teeth = 34;
  const depth = 2.6;
  c.save();
  c.globalCompositeOperation = "destination-out";
  c.fillStyle = "#000";
  for (const edge of [0, UNITS_TALL]) {
    const inward = edge === 0 ? 1 : -1;
    c.beginPath();
    c.moveTo(-1, edge - inward);
    for (let tooth = 0; tooth <= teeth; tooth += 1) {
      const x = (tooth / teeth) * UNITS_WIDE;
      c.lineTo(x, edge - inward);
      if (tooth < teeth)
        c.lineTo(x + UNITS_WIDE / teeth / 2, edge + inward * depth);
    }
    c.lineTo(UNITS_WIDE + 1, edge - inward);
    c.closePath();
    c.fill();
  }
  c.restore();
}

// ─── Colour ─────────────────────────────────────────────────────────────────

/** "#rrggbb" (or "#rrggbbaa") as its three channels. */
function channels(hex: string): [number, number, number] {
  const value = hex.replace("#", "");
  return [0, 2, 4].map((at) => parseInt(value.slice(at, at + 2), 16)) as [
    number,
    number,
    number,
  ];
}
/** A palette colour at an opacity. */
const tint = (hex: string, alpha: number): string =>
  `rgba(${channels(hex).join(",")},${alpha})`;
/** Two palette colours mixed, `amount` of the way from the first, as "#rrggbb". */
function blend(from: string, to: string, amount: number): string {
  const a = channels(from);
  const b = channels(to);
  return `#${a
    .map((value, index) =>
      Math.round(value + ((b[index] ?? 0) - value) * amount)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

/**
 * Paint on a separate layer in the same coordinates, then lay it down.
 * Lets a gradient be shaped by another (rays fading with distance).
 */
function layer(
  c: CanvasRenderingContext2D,
  paint: (layer: CanvasRenderingContext2D) => void,
): void {
  const element = document.createElement("canvas");
  element.width = c.canvas.width;
  element.height = c.canvas.height;
  const context = element.getContext("2d");
  if (!context) return;
  context.setTransform(c.getTransform());
  paint(context);
  c.save();
  c.setTransform(1, 0, 0, 1, 0, 0);
  c.drawImage(element, 0, 0);
  c.restore();
}

/**
 * A sunburst of soft rays around the set symbol, faded with distance: a
 * conic gradient shaped by a radial one.
 */
function paintRays(
  c: CanvasRenderingContext2D,
  color: (alpha: number) => string,
  reach: number,
): void {
  if (!("createConicGradient" in c)) return;
  layer(c, (rays) => {
    const conic = rays.createConicGradient(-Math.PI / 2, EMBLEM.x, EMBLEM.y);
    // A few broad, soft rays: a glow with direction, not a starburst.
    const count = 12;
    for (let ray = 0; ray < count; ray += 1) {
      conic.addColorStop(ray / count, color(0));
      conic.addColorStop((ray + 0.5) / count, color(1));
    }
    conic.addColorStop(1, color(0));
    rays.fillStyle = conic;
    rays.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
    rays.globalCompositeOperation = "destination-in";
    const fade = rays.createRadialGradient(
      EMBLEM.x,
      EMBLEM.y,
      EMBLEM.size * 0.3,
      EMBLEM.x,
      EMBLEM.y,
      reach,
    );
    fade.addColorStop(0, "#000");
    fade.addColorStop(1, "#0000");
    rays.fillStyle = fade;
    rays.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  });
}

function paintField(
  c: CanvasRenderingContext2D,
  palette: PackPalette,
  side: "front" | "back",
): void {
  const angle = ((side === "front" ? 172 : 188) * Math.PI) / 180;
  const dx = Math.sin(angle) * UNITS_TALL * 0.5;
  const dy = -Math.cos(angle) * UNITS_TALL * 0.5;
  const field = c.createLinearGradient(119 - dx, 210 - dy, 119 + dx, 210 + dy);
  field.addColorStop(0, blend(palette.hue, "#ffffff", 0.16));
  field.addColorStop(0.34, palette.hue);
  field.addColorStop(0.7, palette.deep);
  field.addColorStop(1, blend(palette.deep, "#000000", 0.3));
  c.fillStyle = field;
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  // A printed ombré band crossing the foil on the diagonal.
  const band = c.createLinearGradient(0, 70, UNITS_WIDE, 250);
  band.addColorStop(0, tint(palette.hue, 0));
  band.addColorStop(0.42, tint(blend(palette.hue, "#ffffff", 0.45), 0.3));
  band.addColorStop(0.5, tint(blend(palette.hue, "#ffffff", 0.6), 0.42));
  band.addColorStop(0.6, tint(blend(palette.hue, "#ffffff", 0.45), 0.24));
  band.addColorStop(1, tint(palette.hue, 0));
  c.fillStyle = band;
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  const glow = c.createRadialGradient(119, 160, 0, 119, 160, 150);
  glow.addColorStop(0, palette.glow);
  glow.addColorStop(1, "#0000");
  c.fillStyle = glow;
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  const pool = c.createRadialGradient(48, 420, 0, 48, 420, 220);
  pool.addColorStop(0, palette.deep);
  pool.addColorStop(1, "#0000");
  c.fillStyle = pool;
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  // The set's glow rising from the lower crimp.
  const rise = c.createLinearGradient(0, UNITS_TALL - CRIMP, 0, 250);
  rise.addColorStop(0, tint(palette.glow, 0.32));
  rise.addColorStop(1, tint(palette.glow, 0));
  c.fillStyle = rise;
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  // Fine engraved hatching across the whole field.
  c.save();
  c.globalAlpha = 0.07;
  c.strokeStyle = "#fff";
  c.lineWidth = 0.3;
  for (let line = -UNITS_TALL; line < UNITS_WIDE + UNITS_TALL; line += 3.2) {
    c.beginPath();
    c.moveTo(line, 0);
    c.lineTo(line - UNITS_TALL * 0.55, UNITS_TALL);
    c.stroke();
  }
  c.restore();
  // Inner shading towards the folds, as on the 2D wrapper.
  const fold = c.createLinearGradient(0, 0, UNITS_WIDE, 0);
  fold.addColorStop(0, "#0008");
  fold.addColorStop(0.1, "#0000");
  fold.addColorStop(0.9, "#0000");
  fold.addColorStop(1, "#0008");
  c.fillStyle = fold;
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
}

function paintCrimps(c: CanvasRenderingContext2D, palette: PackPalette): void {
  for (const top of [0, UNITS_TALL - CRIMP]) {
    const band = c.createLinearGradient(0, top, 0, top + CRIMP);
    band.addColorStop(0, blend(palette.hue, "#ffffff", 0.22));
    band.addColorStop(0.45, palette.hue);
    band.addColorStop(1, palette.deep);
    c.fillStyle = band;
    c.fillRect(0, top, UNITS_WIDE, CRIMP);
    // Brighter across the middle, falling off towards the folds.
    const across = c.createLinearGradient(0, 0, UNITS_WIDE, 0);
    across.addColorStop(0, "#00000059");
    across.addColorStop(0.5, "#ffffff21");
    across.addColorStop(1, "#00000059");
    c.fillStyle = across;
    c.fillRect(0, top, UNITS_WIDE, CRIMP);
    c.fillStyle = "#0000004d";
    for (let x = 0; x < UNITS_WIDE; x += UNITS_WIDE / 46)
      c.fillRect(x + 2.4, top, 2.2, CRIMP);
    // The seal line where the crimp meets the pillow.
    c.fillStyle = "#00000066";
    c.fillRect(0, top === 0 ? CRIMP - 0.8 : top, UNITS_WIDE, 0.8);
  }
}

function paintFront(
  c: CanvasRenderingContext2D,
  print: PackPrint,
  symbol: SetSymbol | null,
  scale: number,
): void {
  paintField(c, print.palette, "front");
  // Light pooled behind the set symbol, in the set's glow.
  const orb = c.createRadialGradient(
    EMBLEM.x - 11,
    EMBLEM.y - 18,
    0,
    EMBLEM.x,
    EMBLEM.y,
    EMBLEM.glow,
  );
  orb.addColorStop(0, "#ffffff3d");
  orb.addColorStop(0.45, print.palette.glow);
  orb.addColorStop(1, "#0000");
  c.fillStyle = orb;
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  const ray = blend(print.palette.glow, "#ffffff", 0.35);
  paintRays(c, (alpha) => tint(ray, alpha * 0.26), 125);
  if (symbol) {
    c.save();
    c.shadowColor = "#000a";
    c.shadowBlur = 5 * scale;
    c.shadowOffsetY = 2 * scale;
    stamp(c, symbol, goldFoil, EMBLEM, scale);
    c.restore();
  }
  c.textBaseline = "alphabetic";
  const layout = layoutLabel(c, print);
  // A dark band seats the label, edged in gold hairlines.
  const bandTop = (layout.titleY[0] ?? 0) - layout.title.size - 12;
  const bandBottom = layout.kindY + 12;
  const seat = c.createLinearGradient(0, 0, UNITS_WIDE, 0);
  seat.addColorStop(0, tint(print.palette.deep, 0));
  seat.addColorStop(0.22, tint(print.palette.deep, 0.62));
  seat.addColorStop(0.78, tint(print.palette.deep, 0.62));
  seat.addColorStop(1, tint(print.palette.deep, 0));
  c.fillStyle = seat;
  c.fillRect(0, bandTop, UNITS_WIDE, bandBottom - bandTop);
  for (const edge of [bandTop, bandBottom]) {
    const hairline = c.createLinearGradient(0, 0, UNITS_WIDE, 0);
    hairline.addColorStop(0, "#f2dba600");
    hairline.addColorStop(0.5, "#f2dba6b3");
    hairline.addColorStop(1, "#f2dba600");
    c.fillStyle = hairline;
    c.fillRect(0, edge - 0.3, UNITS_WIDE, 0.6);
  }
  const lettering = c.createLinearGradient(0, bandTop, 0, layout.ruleY);
  lettering.addColorStop(0, "#fffaf0");
  lettering.addColorStop(1, "#ecd6a8");
  c.fillStyle = lettering;
  c.font = `${layout.title.size}px ${print.serif}`;
  c.shadowColor = "#000c";
  c.shadowBlur = 6;
  c.shadowOffsetY = 2;
  layout.title.lines.forEach((line, index) =>
    spaced(c, line, 119, layout.titleY[index] ?? 0, layout.title.size * 0.02),
  );
  c.shadowColor = "#0000";
  c.fillStyle = GOLD;
  traceRule(c, layout.ruleY);
  if (layout.kind) {
    c.fillStyle = CREAM;
    c.font = `13px ${print.serif}`;
    spaced(c, layout.kind, 119, layout.kindY, 13 * 0.24);
  }
  paintCrimps(c, print.palette);
  cutSerration(c);
}

function paintBack(
  c: CanvasRenderingContext2D,
  print: PackPrint,
  symbol: SetSymbol | null,
  scale: number,
): void {
  paintField(c, print.palette, "back");
  // The fin seal running down the back.
  const fin = c.createLinearGradient(107, 0, 131, 0);
  fin.addColorStop(0, "#0000");
  fin.addColorStop(0.2, "#00000059");
  fin.addColorStop(0.5, "#ffffff1a");
  fin.addColorStop(0.8, "#00000059");
  fin.addColorStop(1, "#0000");
  c.fillStyle = fin;
  c.fillRect(107, 0, 24, UNITS_TALL);
  if (symbol) stamp(c, symbol, goldFoil, BACK_EMBLEM, scale);
  c.fillStyle = GOLD;
  traceRule(c, BACK_RULE_Y, 0.42);
  c.fillStyle = CREAM;
  c.font = `11px ${print.serif}`;
  spaced(c, print.setName.toUpperCase(), 119, BACK_NAME_Y, 1.6);
  paintCrimps(c, print.palette);
  cutSerration(c);
}

// ─── Relief ─────────────────────────────────────────────────────────────────

/** Height field: mid-grey rest, white raised, black pressed. */
function paintRelief(
  c: CanvasRenderingContext2D,
  print: PackPrint,
  side: "front" | "back",
  symbol: SetSymbol | null,
  scale: number,
): void {
  const random = stream(`${print.setName}:${side}`);
  c.fillStyle = "#808080";
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  // Broad dents: slow undulation that makes reflections wander like real foil.
  for (let dent = 0; dent < 22; dent += 1) {
    const x = random() * UNITS_WIDE;
    const y = CRIMP + random() * (UNITS_TALL - CRIMP * 2);
    const radius = 26 + random() * 60;
    const lift = random() < 0.5 ? "255,255,255" : "0,0,0";
    const dentGradient = c.createRadialGradient(x, y, 0, x, y, radius);
    dentGradient.addColorStop(0, `rgba(${lift},0.13)`);
    dentGradient.addColorStop(1, `rgba(${lift},0)`);
    c.fillStyle = dentGradient;
    c.fillRect(x - radius, y - radius, radius * 2, radius * 2);
  }
  // Soft folds where the pillow gathers into each crimp: broad, rounded
  // swells rather than lines, so the shine bends instead of breaking.
  for (const [from, direction] of [
    [CRIMP, 1],
    [UNITS_TALL - CRIMP, -1],
  ] as const) {
    for (let fold = 0; fold < 9; fold += 1) {
      const x = 14 + (fold / 8) * (UNITS_WIDE - 28) + (random() - 0.5) * 10;
      const reach = 22 + random() * 26;
      const lean = ((x - 119) / 119) * 0.35;
      c.save();
      c.translate(x, from + direction * reach * 0.5);
      c.rotate(lean * direction);
      c.scale(1, reach / 7);
      const swell = c.createRadialGradient(0, 0, 0, 0, 0, 7);
      const lift = fold % 2 === 0 ? "255,255,255" : "0,0,0";
      swell.addColorStop(0, `rgba(${lift},0.16)`);
      swell.addColorStop(1, `rgba(${lift},0)`);
      c.fillStyle = swell;
      c.fillRect(-7, -7, 14, 14);
      c.restore();
    }
  }
  // Embossing: the set symbol and the label stand proud of the foil.
  const raised = "rgba(255,255,255,0.7)";
  c.fillStyle = raised;
  c.strokeStyle = raised;
  if (side === "front") {
    if (symbol) stamp(c, symbol, flat(raised), EMBLEM, scale);
    c.fillStyle = raised;
    const layout = layoutLabel(c, print);
    c.font = `${layout.title.size}px ${print.serif}`;
    layout.title.lines.forEach((line, index) =>
      spaced(c, line, 119, layout.titleY[index] ?? 0, layout.title.size * 0.02),
    );
    traceRule(c, layout.ruleY);
  } else {
    if (symbol) stamp(c, symbol, flat(raised), BACK_EMBLEM, scale);
    // Fin seal: a raised strip with pressed bars across it.
    c.fillStyle = "rgba(255,255,255,0.4)";
    c.fillRect(110, CRIMP, 18, UNITS_TALL - CRIMP * 2);
    c.fillStyle = "rgba(0,0,0,0.3)";
    for (let y = CRIMP; y < UNITS_TALL - CRIMP; y += 4.2)
      c.fillRect(110, y, 18, 1.6);
  }
  // Crimps: deep vertical ridges, pressed below the pillow.
  for (const top of [0, UNITS_TALL - CRIMP]) {
    const period = UNITS_WIDE / 46;
    for (let x = 0; x < UNITS_WIDE; x += 0.5) {
      const ridge = 0.5 + 0.5 * Math.cos((x / period) * Math.PI * 2);
      const level = Math.round(40 + ridge * 175);
      c.fillStyle = `rgb(${level},${level},${level})`;
      c.fillRect(x, top, 0.5, CRIMP);
    }
  }
}

/** Box-blur a height channel in place, in both directions. */
function blur(
  heights: Float32Array,
  width: number,
  height: number,
  radius: number,
): void {
  const scratch = new Float32Array(heights.length);
  const span = radius * 2 + 1;
  for (let y = 0; y < height; y += 1) {
    let sum = 0;
    for (let x = -radius; x <= radius; x += 1)
      sum += heights[y * width + Math.min(width - 1, Math.max(0, x))] ?? 0;
    for (let x = 0; x < width; x += 1) {
      scratch[y * width + x] = sum / span;
      const out = heights[y * width + Math.max(0, x - radius)] ?? 0;
      const into =
        heights[y * width + Math.min(width - 1, x + radius + 1)] ?? 0;
      sum += into - out;
    }
  }
  for (let x = 0; x < width; x += 1) {
    let sum = 0;
    for (let y = -radius; y <= radius; y += 1)
      sum += scratch[Math.min(height - 1, Math.max(0, y)) * width + x] ?? 0;
    for (let y = 0; y < height; y += 1) {
      heights[y * width + x] = sum / span;
      const out = scratch[Math.max(0, y - radius) * width + x] ?? 0;
      const into =
        scratch[Math.min(height - 1, y + radius + 1) * width + x] ?? 0;
      sum += into - out;
    }
  }
}

/**
 * Turn a painted height field into a tangent-space normal map. `grain` adds
 * a fine, even tooth across the foil, which glitters where light catches it.
 */
function reliefToNormals(
  element: HTMLCanvasElement,
  strength: number,
  grain: number,
  seed: string,
): void {
  const context = element.getContext("2d");
  if (!context) return;
  const { width, height } = element;
  const image = context.getImageData(0, 0, width, height);
  const heights = new Float32Array(width * height);
  for (let index = 0; index < heights.length; index += 1)
    heights[index] = (image.data[index * 4] ?? 128) / 255;
  blur(heights, width, height, Math.max(1, Math.round(width / 700)));
  if (grain > 0) {
    const random = stream(seed);
    for (let index = 0; index < heights.length; index += 1)
      heights[index] = (heights[index] ?? 0) + (random() - 0.5) * grain;
  }
  const scale = strength * (width / 1024);
  const at = (x: number, y: number): number =>
    heights[
      Math.min(height - 1, Math.max(0, y)) * width +
        Math.min(width - 1, Math.max(0, x))
    ] ?? 0;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      // Canvas rows run down the texture; v runs up.
      const nx = -(at(x + 1, y) - at(x - 1, y)) * scale;
      const ny = (at(x, y + 1) - at(x, y - 1)) * scale;
      const length = Math.hypot(nx, ny, 1);
      const index = (y * width + x) * 4;
      image.data[index] = ((nx / length) * 0.5 + 0.5) * 255;
      image.data[index + 1] = ((ny / length) * 0.5 + 0.5) * 255;
      image.data[index + 2] = ((1 / length) * 0.5 + 0.5) * 255;
      image.data[index + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
}

// ─── Finish ─────────────────────────────────────────────────────────────────

/** R iridescence, G roughness, B metalness. */
function paintFinish(
  c: CanvasRenderingContext2D,
  print: PackPrint,
  side: "front" | "back",
  symbol: SetSymbol | null,
  scale: number,
): void {
  const random = stream(`${print.setName}:${side}:finish`);
  // Foil field: metallic, satin.
  c.fillStyle = "rgb(0,70,226)";
  c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
  // Roughness wanders a little, like handled foil.
  for (let patch = 0; patch < 30; patch += 1) {
    const x = random() * UNITS_WIDE;
    const y = random() * UNITS_TALL;
    const radius = 20 + random() * 50;
    const gradient = c.createRadialGradient(x, y, 0, x, y, radius);
    const g = random() < 0.5 ? 48 : 104;
    gradient.addColorStop(0, `rgba(0,${g},226,0.5)`);
    gradient.addColorStop(1, "rgba(0,70,226,0)");
    c.fillStyle = gradient;
    c.fillRect(x - radius, y - radius, radius * 2, radius * 2);
  }
  const gilt = flat("rgb(30,40,255)");
  // Cream ink: matte, barely metallic.
  const ink = "rgb(0,150,40)";
  if (side === "front") {
    // Holographic glow behind the set symbol.
    const holo = c.createRadialGradient(
      EMBLEM.x,
      EMBLEM.y,
      0,
      EMBLEM.x,
      EMBLEM.y,
      EMBLEM.glow,
    );
    holo.addColorStop(0, "rgba(255,58,240,1)");
    holo.addColorStop(0.8, "rgba(200,70,236,0.8)");
    holo.addColorStop(1, "rgba(0,70,226,0)");
    c.fillStyle = holo;
    c.fillRect(0, 0, UNITS_WIDE, UNITS_TALL);
    // The sunburst's rays shimmer holographically.
    paintRays(c, (alpha) => `rgba(255,50,240,${alpha * 0.45})`, 120);
    // Gold stamping: bright and smooth.
    if (symbol) stamp(c, symbol, gilt, EMBLEM, scale);
    const layout = layoutLabel(c, print);
    c.fillStyle = gilt();
    traceRule(c, layout.ruleY);
    c.fillStyle = ink;
    c.font = `${layout.title.size}px ${print.serif}`;
    layout.title.lines.forEach((line, index) =>
      spaced(c, line, 119, layout.titleY[index] ?? 0, layout.title.size * 0.02),
    );
    if (layout.kind) {
      c.font = `13px ${print.serif}`;
      spaced(c, layout.kind, 119, layout.kindY, 13 * 0.24);
    }
  } else {
    if (symbol) stamp(c, symbol, gilt, BACK_EMBLEM, scale);
    c.fillStyle = ink;
    c.font = `11px ${print.serif}`;
    spaced(c, print.setName.toUpperCase(), 119, BACK_NAME_Y, 1.6);
    c.fillStyle = gilt();
    traceRule(c, BACK_RULE_Y, 0.42);
  }
  // Crimps: pressed, less mirror-like.
  c.fillStyle = "rgb(0,120,200)";
  c.fillRect(0, 0, UNITS_WIDE, CRIMP);
  c.fillRect(0, UNITS_TALL - CRIMP, UNITS_WIDE, CRIMP);
}

function texture(
  element: HTMLCanvasElement,
  colorSpace: THREE.ColorSpace,
  anisotropy: number,
): THREE.CanvasTexture {
  const map = new THREE.CanvasTexture(element);
  map.colorSpace = colorSpace;
  map.anisotropy = anisotropy;
  map.wrapS = THREE.ClampToEdgeWrapping;
  map.wrapT = THREE.ClampToEdgeWrapping;
  map.needsUpdate = true;
  return map;
}

/**
 * Print both sides. The front carries the detail and gets the larger
 * canvases; the back is mostly seen in passing. With `back: false` the
 * front is used for both, for views that never turn the pack round.
 */
export async function printPack(
  print: PackPrint,
  options: {
    anisotropy: number;
    detail: "full" | "reduced";
    back?: boolean;
    signal?: AbortSignal;
  },
): Promise<{ front: SidePrint; back: SidePrint; dispose: () => void }> {
  // A missing symbol leaves the emblem to the set's glow and name; nothing stands in for it.
  const symbol = print.symbolUrl
    ? await loadSymbol(print.symbolUrl, options.signal).catch(
        (error: unknown) => {
          options.signal?.throwIfAborted();
          console.warn("Printing the pack without its set symbol.", error);
          return null;
        },
      )
    : null;
  options.signal?.throwIfAborted();
  const large = options.detail === "full" ? 1024 : 640;
  const small = options.detail === "full" ? 512 : 384;
  const make = (side: "front" | "back"): SidePrint => {
    const colorWidth = side === "front" ? large : small;
    const color = canvas(colorWidth);
    if (side === "front") paintFront(color.context, print, symbol, color.scale);
    else paintBack(color.context, print, symbol, color.scale);
    const relief = canvas(colorWidth);
    paintRelief(relief.context, print, side, symbol, relief.scale);
    reliefToNormals(
      relief.element,
      3.2,
      0.012,
      `${print.setName}:${side}:grain`,
    );
    const finish = canvas(small);
    paintFinish(finish.context, print, side, symbol, finish.scale);
    return {
      color: texture(color.element, THREE.SRGBColorSpace, options.anisotropy),
      normal: texture(relief.element, THREE.NoColorSpace, options.anisotropy),
      finish: texture(finish.element, THREE.NoColorSpace, options.anisotropy),
    };
  };
  const front = make("front");
  const back = options.back === false ? front : make("back");
  return {
    front,
    back,
    dispose: () => {
      for (const side of new Set([front, back])) {
        side.color.dispose();
        side.normal.dispose();
        side.finish.dispose();
      }
    },
  };
}

/** A soft round sprite for glows, light and shadow. */
export function softSprite(
  inner: string,
  outer: string,
  size = 128,
): THREE.CanvasTexture {
  const element = document.createElement("canvas");
  element.width = size;
  element.height = size;
  const context = element.getContext("2d");
  if (!context) throw new Error("A 2D canvas is needed for the pack's light.");
  const gradient = context.createRadialGradient(
    size / 2,
    size / 2,
    0,
    size / 2,
    size / 2,
    size / 2,
  );
  gradient.addColorStop(0, inner);
  gradient.addColorStop(1, outer);
  context.fillStyle = gradient;
  context.fillRect(0, 0, size, size);
  const map = new THREE.CanvasTexture(element);
  map.colorSpace = THREE.SRGBColorSpace;
  return map;
}

/** A tall soft ray, brightest at its base, for light leaving the pack. */
export function raySprite(): THREE.CanvasTexture {
  const element = document.createElement("canvas");
  element.width = 64;
  element.height = 256;
  const context = element.getContext("2d");
  if (!context) throw new Error("A 2D canvas is needed for the pack's light.");
  const across = context.createLinearGradient(0, 0, 64, 0);
  across.addColorStop(0, "rgba(255,255,255,0)");
  across.addColorStop(0.5, "rgba(255,255,255,1)");
  across.addColorStop(1, "rgba(255,255,255,0)");
  context.fillStyle = across;
  context.fillRect(0, 0, 64, 256);
  context.globalCompositeOperation = "destination-in";
  const along = context.createLinearGradient(0, 256, 0, 0);
  along.addColorStop(0, "rgba(0,0,0,1)");
  along.addColorStop(0.15, "rgba(0,0,0,0.85)");
  along.addColorStop(1, "rgba(0,0,0,0)");
  context.fillStyle = along;
  context.fillRect(0, 0, 64, 256);
  const map = new THREE.CanvasTexture(element);
  map.colorSpace = THREE.SRGBColorSpace;
  return map;
}
