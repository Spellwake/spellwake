/**
 * Unified 300-DPI (1062x1482) procedural SVG card frame renderer for Spellwake.
 *
 * Supports both full custom cards (`kind: "card_face"`) across all Spellwake layouts
 * (normal, transform, modal_dfc, adventure, split, planeswalker, saga) and compact
 * stack ability cards (`kind: "stack_trigger"`) using the exact same 1062x1482
 * M15-proportioned geometry and bundled 84 Scryfall vector mana/game symbols.
 */

import { lookupManaSymbolSvg } from "./mana-symbols";

export const CARD_WIDTH = 1062;
export const CARD_HEIGHT = 1482;

export type CardFrameAccent =
  | "auto"
  | "W"
  | "U"
  | "B"
  | "R"
  | "G"
  | "gold"
  | "artifact"
  | "colorless"
  | "land";

export type CardLayoutKind =
  | "normal"
  | "transform"
  | "modal_dfc"
  | "adventure"
  | "split"
  | "planeswalker"
  | "saga";

export type CardRarityKind = "common" | "uncommon" | "rare" | "mythic";

export interface CardFaceRenderData {
  name: string;
  manaCost?: string | null;
  typeLine: string;
  oracleText: string;
  flavorText?: string | null;
  power?: string | null;
  toughness?: string | null;
  loyalty?: string | null;
  defense?: string | null;
  colorIndicator?: string[] | null;
  artist?: string | null;
  artUrl?: string | null;
  cropX?: number;
  cropY?: number;
  zoom?: number;
}

export interface CardFaceFrameInput {
  kind: "card_face";
  layout: CardLayoutKind;
  rarity: CardRarityKind;
  frameAccent?: CardFrameAccent;
  face: CardFaceRenderData;
  secondaryFace?: CardFaceRenderData | null;
  faceIndex?: 0 | 1;
  collectorNote?: string | null;
  /** Optional suffix for unique SVG `<defs>` IDs when multiple cards render in one DOM. */
  idPrefix?: string;
}

export interface StackTriggerFrameInput {
  kind: "stack_trigger";
  name: string;
  typeLabel: string;
  effect: string;
  artUrl?: string | null;
  /**
   * Whether `artUrl` is already cropped artwork (`"art"`, default) or a full
   * 5:7 printed card face scan (`"full_card"`) that should be cropped into the
   * standard Magic artwork window.
   */
  artSourceKind?: "art" | "full_card";
  illustrated?: boolean;
  frameAccent?: CardFrameAccent;
  idPrefix?: string;
}

export type CardFrameRenderInput = CardFaceFrameInput | StackTriggerFrameInput;

export interface FramePalette {
  key: string;
  frameTop: string;
  frameBottom: string;
  pinlineStart: string;
  pinlineEnd: string;
  titleFill: string;
  titleStroke: string;
  titleInk: string;
  boxFill: string;
  boxAltFill: string;
  boxInk: string;
  ptFill: string;
  ptInk: string;
  accentGlow: string;
}

const PALETTES: Record<Exclude<CardFrameAccent, "auto">, FramePalette> = {
  W: {
    key: "W",
    frameTop: "#ebe3cf",
    frameBottom: "#cfc1a1",
    pinlineStart: "#f5efdf",
    pinlineEnd: "#c8b383",
    titleFill: "#f7f2e7",
    titleStroke: "#b9a479",
    titleInk: "#171410",
    boxFill: "#f8f4ea",
    boxAltFill: "#ede6d6",
    boxInk: "#161411",
    ptFill: "#ede4ce",
    ptInk: "#15130f",
    accentGlow: "#f5e3b0",
  },
  U: {
    key: "U",
    frameTop: "#20507a",
    frameBottom: "#122b44",
    pinlineStart: "#57a3dc",
    pinlineEnd: "#24669c",
    titleFill: "#d6e5f2",
    titleStroke: "#4889bf",
    titleInk: "#0e1720",
    boxFill: "#e4eef6",
    boxAltFill: "#d4e3ef",
    boxInk: "#0f1720",
    ptFill: "#d2e2f0",
    ptInk: "#0e1720",
    accentGlow: "#6bb4ec",
  },
  B: {
    key: "B",
    frameTop: "#332e38",
    frameBottom: "#19161d",
    pinlineStart: "#887c94",
    pinlineEnd: "#4c4355",
    titleFill: "#d6d1dc",
    titleStroke: "#6d6278",
    titleInk: "#141118",
    boxFill: "#e3dfe7",
    boxAltFill: "#d5cfd9",
    boxInk: "#151218",
    ptFill: "#cfc8d6",
    ptInk: "#141118",
    accentGlow: "#a292b2",
  },
  R: {
    key: "R",
    frameTop: "#7c2c1d",
    frameBottom: "#42150e",
    pinlineStart: "#de6a48",
    pinlineEnd: "#99341e",
    titleFill: "#f1dad1",
    titleStroke: "#bf583a",
    titleInk: "#1b0d09",
    boxFill: "#f5e6df",
    boxAltFill: "#ead4ca",
    boxInk: "#1a0e0a",
    ptFill: "#ebd2c8",
    ptInk: "#1b0d09",
    accentGlow: "#ec7f57",
  },
  G: {
    key: "G",
    frameTop: "#215534",
    frameBottom: "#112f1c",
    pinlineStart: "#59a870",
    pinlineEnd: "#276b3e",
    titleFill: "#d7e7db",
    titleStroke: "#448a5b",
    titleInk: "#0d1a11",
    boxFill: "#e4efe6",
    boxAltFill: "#d5e4d8",
    boxInk: "#0e1911",
    ptFill: "#d0e2d5",
    ptInk: "#0d1a11",
    accentGlow: "#71c188",
  },
  gold: {
    key: "gold",
    frameTop: "#8b6d34",
    frameBottom: "#4c3815",
    pinlineStart: "#e6c675",
    pinlineEnd: "#a98237",
    titleFill: "#efe3c2",
    titleStroke: "#c7a354",
    titleInk: "#1b1509",
    boxFill: "#f4ebd3",
    boxAltFill: "#e9dcc0",
    boxInk: "#19140a",
    ptFill: "#e9d9b1",
    ptInk: "#1b1509",
    accentGlow: "#f3d589",
  },
  artifact: {
    key: "artifact",
    frameTop: "#606d78",
    frameBottom: "#343d45",
    pinlineStart: "#b6c5d1",
    pinlineEnd: "#6e7f8c",
    titleFill: "#dfe6eb",
    titleStroke: "#8e9fad",
    titleInk: "#12171b",
    boxFill: "#e9eef2",
    boxAltFill: "#dce3e8",
    boxInk: "#13171b",
    ptFill: "#d8e1e8",
    ptInk: "#12171b",
    accentGlow: "#c2d4e2",
  },
  colorless: {
    key: "colorless",
    frameTop: "#363c46",
    frameBottom: "#1c2028",
    pinlineStart: "#9ba6b6",
    pinlineEnd: "#5d6675",
    titleFill: "#e0e5ec",
    titleStroke: "#838e9e",
    titleInk: "#11151b",
    boxFill: "#ebeef3",
    boxAltFill: "#dfe4ec",
    boxInk: "#12161c",
    ptFill: "#d7dde6",
    ptInk: "#11151b",
    accentGlow: "#b4c0d2",
  },
  land: {
    key: "land",
    frameTop: "#6a5a46",
    frameBottom: "#3a3024",
    pinlineStart: "#c3ad8d",
    pinlineEnd: "#7d684d",
    titleFill: "#ebe2d3",
    titleStroke: "#a48e6f",
    titleInk: "#1a150f",
    boxFill: "#f1eae0",
    boxAltFill: "#e5dbcd",
    boxInk: "#1a150f",
    ptFill: "#e5dac8",
    ptInk: "#1a150f",
    accentGlow: "#d7c09c",
  },
};

const COLOR_HEX: Record<string, string> = {
  W: "#f2e7c9",
  U: "#4795d1",
  B: "#6e617c",
  R: "#d65d3f",
  G: "#43945f",
};

const RARITY_STYLES: Record<CardRarityKind, { fill: string; stroke: string; glint: string }> = {
  common: { fill: "#1b1d22", stroke: "#d9dde4", glint: "#525864" },
  uncommon: { fill: "#8b9db0", stroke: "#181d22", glint: "#dde7f0" },
  rare: { fill: "#c99f3c", stroke: "#1c1506", glint: "#f8e29b" },
  mythic: { fill: "#d45420", stroke: "#220a03", glint: "#f7a44a" },
};

export function extractColorsFromFace(face: CardFaceRenderData): Array<"W" | "U" | "B" | "R" | "G"> {
  const found = new Set<"W" | "U" | "B" | "R" | "G">();
  if (face.colorIndicator && face.colorIndicator.length > 0) {
    for (const c of face.colorIndicator) {
      if (c === "W" || c === "U" || c === "B" || c === "R" || c === "G") {
        found.add(c);
      }
    }
  }
  const mana = face.manaCost ?? "";
  const regex = /\{([^}]+)\}/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(mana)) !== null) {
    const token = (match[1] ?? "").toUpperCase();
    for (const part of token.split("/")) {
      if (part === "W" || part === "U" || part === "B" || part === "R" || part === "G") {
        found.add(part);
      }
    }
  }
  return (["W", "U", "B", "R", "G"] as const).filter((c) => found.has(c));
}

export function resolveFramePalette(
  face: CardFaceRenderData,
  frameAccent: CardFrameAccent = "auto",
): FramePalette {
  if (frameAccent !== "auto") {
    return PALETTES[frameAccent];
  }
  const typeLower = face.typeLine.toLowerCase();
  if (typeLower.includes("land")) {
    return PALETTES.land;
  }
  const colors = extractColorsFromFace(face);
  const c0 = colors[0];
  const c1 = colors[1];
  if (colors.length === 1 && c0) {
    return PALETTES[c0];
  }
  if (colors.length === 2 && c0 && c1) {
    const base = PALETTES.gold;
    return {
      ...base,
      key: `${c0}${c1}`,
      frameTop: PALETTES[c0].frameTop,
      frameBottom: PALETTES[c1].frameBottom,
      pinlineStart: COLOR_HEX[c0] ?? base.pinlineStart,
      pinlineEnd: COLOR_HEX[c1] ?? base.pinlineEnd,
    };
  }
  if (colors.length >= 3) {
    return PALETTES.gold;
  }
  if (typeLower.includes("artifact")) {
    return PALETTES.artifact;
  }
  return PALETTES.colorless;
}

export function escapeXml(raw: string): string {
  return raw
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

/** Parse a mana cost string like `{2}{U}{R}` into an array of canonical tokens. */
export function parseManaTokens(manaCost: string | null | undefined): string[] {
  if (!manaCost) return [];
  const tokens: string[] = [];
  const regex = /\{([^}]+)\}/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(manaCost)) !== null) {
    tokens.push(`{${(match[1] ?? "").trim().toUpperCase()}}`);
  }
  return tokens;
}

/** Embed a Scryfall symbol SVG scaled cleanly to `size` px at `(x, y)`. */
function renderInlineSymbol(token: string, x: number, y: number, size: number, shadow = false): string {
  const rawSvg = lookupManaSymbolSvg(token);
  if (!rawSvg) {
    const label = escapeXml(token.replace(/[{}]/g, ""));
    return `<g transform="translate(${x.toFixed(1)},${y.toFixed(1)})">
      <circle cx="${(size / 2).toFixed(1)}" cy="${(size / 2).toFixed(1)}" r="${(size / 2).toFixed(1)}" fill="#cac5c0" stroke="#181512" stroke-width="1.8"/>
      <text x="${(size / 2).toFixed(1)}" y="${(size * 0.72).toFixed(1)}" text-anchor="middle" font-family="Georgia, serif" font-weight="700" font-size="${(size * 0.62).toFixed(1)}" fill="#14110f">${label}</text>
    </g>`;
  }
  const inner = rawSvg
    .replace(/^<svg[^>]*>/i, "")
    .replace(/<\/svg>\s*$/i, "");
  const viewBoxMatch = /viewBox="([^"]+)"/i.exec(rawSvg);
  const viewBox = viewBoxMatch ? viewBoxMatch[1] : "0 0 100 100";
  const shadowCircle = shadow
    ? `<circle cx="${(x + size / 2 - 1).toFixed(1)}" cy="${(y + size / 2 + 2.5).toFixed(1)}" r="${(size / 2).toFixed(1)}" fill="rgba(0,0,0,0.42)"/>`
    : "";
  return `${shadowCircle}<svg x="${x.toFixed(1)}" y="${y.toFixed(1)}" width="${size.toFixed(1)}" height="${size.toFixed(1)}" viewBox="${viewBox}">${inner}</svg>`;
}

/** Render right-aligned mana cost symbols inside the header bar. */
function renderManaCostBar(
  manaCost: string | null | undefined,
  rightX: number,
  centerY: number,
  symbolSize = 44,
  gap = 7,
): { svg: string; totalWidth: number } {
  const tokens = parseManaTokens(manaCost);
  if (tokens.length === 0) return { svg: "", totalWidth: 0 };
  const totalWidth = tokens.length * symbolSize + Math.max(0, tokens.length - 1) * gap;
  let cursor = rightX - totalWidth;
  const parts: string[] = [];
  for (const tok of tokens) {
    parts.push(renderInlineSymbol(tok, cursor, centerY - symbolSize / 2, symbolSize, true));
    cursor += symbolSize + gap;
  }
  return { svg: parts.join(""), totalWidth };
}

interface TextSegment {
  kind: "text" | "symbol";
  value: string;
}

function tokenizeRichLine(line: string): TextSegment[] {
  const segments: TextSegment[] = [];
  const regex = /(\{[^}]+\})/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(line)) !== null) {
    if (match.index > lastIndex) {
      segments.push({ kind: "text", value: line.slice(lastIndex, match.index) });
    }
    segments.push({ kind: "symbol", value: match[1] ?? "" });
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < line.length) {
    segments.push({ kind: "text", value: line.slice(lastIndex) });
  }
  return segments;
}

interface WrappedLine {
  segments: TextSegment[];
  italic?: boolean;
}

function estimateTextWidth(text: string, fontSize: number): number {
  let units = 0;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i] ?? " ";
    if (ch === " ") units += 0.27;
    else if ("ilI1.,;:!'|`()[]".includes(ch)) units += 0.30;
    else if ("mwMW@%".includes(ch)) units += 0.78;
    else if (ch >= "A" && ch <= "Z") units += 0.62;
    else units += 0.48;
  }
  return units * fontSize;
}

function wrapParagraph(text: string, maxWidth: number, fontSize: number, italic = false): WrappedLine[] {
  const symbolW = fontSize * 0.94;
  const tokens: TextSegment[] = [];
  for (const seg of tokenizeRichLine(text)) {
    if (seg.kind === "symbol") {
      tokens.push(seg);
    } else {
      const words = seg.value.split(/(\s+)/);
      for (const w of words) {
        if (w.length > 0) tokens.push({ kind: "text", value: w });
      }
    }
  }

  const lines: WrappedLine[] = [];
  let current: TextSegment[] = [];
  let width = 0;

  for (const tok of tokens) {
    const tokW = tok.kind === "symbol" ? symbolW : estimateTextWidth(tok.value, fontSize);
    if (current.length > 0 && width + tokW > maxWidth && !(tok.kind === "text" && /^\s+$/.test(tok.value))) {
      lines.push({ segments: current, italic });
      current = [];
      width = 0;
      if (tok.kind === "text" && /^\s+$/.test(tok.value)) continue;
    }
    if (current.length === 0 && tok.kind === "text" && /^\s+$/.test(tok.value)) {
      continue;
    }
    current.push(tok);
    width += tokW;
  }
  if (current.length > 0) {
    lines.push({ segments: current, italic });
  }
  return lines;
}

function coalesceLineSegments(segments: TextSegment[]): TextSegment[] {
  const merged: TextSegment[] = [];
  for (const seg of segments) {
    const prev = merged[merged.length - 1];
    if (seg.kind === "text" && prev?.kind === "text") {
      prev.value += seg.value;
    } else {
      merged.push({ ...seg });
    }
  }
  return merged;
}

function renderWrappedRichLines(
  lines: WrappedLine[],
  startX: number,
  startY: number,
  fontSize: number,
  lineHeight: number,
  fill: string,
): string {
  const symbolSize = fontSize * 0.88;
  const out: string[] = [];

  lines.forEach((line, idx) => {
    const baselineY = startY + idx * lineHeight;
    const merged = coalesceLineSegments(line.segments);
    let cursorX = startX;
    for (const seg of merged) {
      if (seg.kind === "symbol") {
        out.push(
          renderInlineSymbol(
            seg.value,
            cursorX,
            baselineY - symbolSize * 0.78,
            symbolSize,
            false,
          ),
        );
        cursorX += symbolSize + 3;
      } else {
        const fontStyle = line.italic ? ' font-style="italic"' : "";
        out.push(
          `<text x="${cursorX.toFixed(1)}" y="${baselineY.toFixed(1)}" font-family="Baskerville, Georgia, 'Times New Roman', serif" font-size="${fontSize.toFixed(1)}" fill="${fill}"${fontStyle} xml:space="preserve">${escapeXml(seg.value)}</text>`,
        );
        cursorX += estimateTextWidth(seg.value, fontSize);
        if (seg.value.endsWith(" ")) {
          cursorX += fontSize * 0.16;
        }
      }
    }
  });
  return out.join("");
}

function renderColorIndicatorDot(colors: string[] | null | undefined, cx: number, cy: number, r: number): string {
  if (!colors || colors.length === 0) return "";
  const valid = colors.filter((c) => c in COLOR_HEX);
  if (valid.length === 0) return "";
  const firstColor = valid[0];
  if (valid.length === 1 && firstColor) {
    return `<circle cx="${cx}" cy="${cy}" r="${r}" fill="${COLOR_HEX[firstColor] ?? "#888"}" stroke="#1a1714" stroke-width="2.5"/>`;
  }
  const slices: string[] = [];
  const step = (Math.PI * 2) / valid.length;
  valid.forEach((c, idx) => {
    const a0 = -Math.PI / 2 + idx * step;
    const a1 = a0 + step;
    const x0 = cx + r * Math.cos(a0);
    const y0 = cy + r * Math.sin(a0);
    const x1 = cx + r * Math.cos(a1);
    const y1 = cy + r * Math.sin(a1);
    const largeArc = step > Math.PI ? 1 : 0;
    slices.push(
      `<path d="M ${cx} ${cy} L ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${largeArc} 1 ${x1.toFixed(2)} ${y1.toFixed(2)} Z" fill="${COLOR_HEX[c]}"/>`,
    );
  });
  return `<g>${slices.join("")}<circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="#1a1714" stroke-width="2.5"/></g>`;
}

function renderRarityEmblem(rarity: CardRarityKind, cx: number, cy: number): string {
  const st = RARITY_STYLES[rarity];
  return `<g transform="translate(${cx},${cy})">
    <path d="M 0 -22 L 17 -6 L 11 20 L 0 13 L -11 20 L -17 -6 Z" fill="${st.fill}" stroke="${st.stroke}" stroke-width="3.2" stroke-linejoin="round"/>
    <path d="M 0 -14 L 8 -4 L 0 9 L -8 -4 Z" fill="${st.glint}" opacity="0.65"/>
  </g>`;
}

function renderArtWindow(
  artUrl: string | null | undefined,
  x: number,
  y: number,
  w: number,
  h: number,
  cropX: number,
  cropY: number,
  zoom: number,
  palette: FramePalette,
  clipId: string,
): string {
  const safeZoom = Math.max(0.5, Math.min(4, Number.isFinite(zoom) ? zoom : 1));
  const safeCx = Math.max(-1, Math.min(1, Number.isFinite(cropX) ? cropX : 0));
  const safeCy = Math.max(-1, Math.min(1, Number.isFinite(cropY) ? cropY : 0));

  if (artUrl && artUrl.trim().length > 0) {
    // Allow smooth drag-to-pan even at default zoom = 1 by giving panned images a subtle bleed margin.
    const isPanned = Math.abs(safeCx) > 0.001 || Math.abs(safeCy) > 0.001;
    const effectiveZoom = isPanned ? Math.max(safeZoom, 1.2) : safeZoom;
    const scaledW = w * effectiveZoom;
    const scaledH = h * effectiveZoom;
    const imgX = x + (w - scaledW) * ((safeCx + 1) / 2);
    const imgY = y + (h - scaledH) * ((safeCy + 1) / 2);
    return `<g clip-path="url(#${clipId})">
      <rect x="${x}" y="${y}" width="${w}" height="${h}" fill="#0d1117"/>
      <image href="${escapeXml(artUrl)}" x="${imgX.toFixed(1)}" y="${imgY.toFixed(1)}" width="${scaledW.toFixed(1)}" height="${scaledH.toFixed(1)}" preserveAspectRatio="xMidYMid slice"/>
    </g>`;
  }

  // Refined dark-atelier placeholder crest when no artwork has been uploaded yet.
  const midX = x + w / 2;
  const midY = y + h / 2;
  return `<g clip-path="url(#${clipId})">
    <rect x="${x}" y="${y}" width="${w}" height="${h}" fill="#10151e"/>
    <radialGradient id="${clipId}-bg" cx="50%" cy="42%" r="65%">
      <stop offset="0%" stop-color="${palette.pinlineStart}" stop-opacity="0.28"/>
      <stop offset="62%" stop-color="${palette.frameTop}" stop-opacity="0.16"/>
      <stop offset="100%" stop-color="#0b0e14" stop-opacity="0.96"/>
    </radialGradient>
    <rect x="${x}" y="${y}" width="${w}" height="${h}" fill="url(#${clipId}-bg)"/>
    <circle cx="${midX}" cy="${midY}" r="${Math.min(w, h) * 0.31}" fill="none" stroke="${palette.accentGlow}" stroke-opacity="0.24" stroke-width="2"/>
    <circle cx="${midX}" cy="${midY}" r="${Math.min(w, h) * 0.22}" fill="none" stroke="${palette.accentGlow}" stroke-opacity="0.16" stroke-width="1.5" stroke-dasharray="8 8"/>
    <path d="M ${midX} ${midY - 96} L ${midX + 68} ${midY} L ${midX} ${midY + 96} L ${midX - 68} ${midY} Z" fill="none" stroke="${palette.accentGlow}" stroke-opacity="0.34" stroke-width="2.5"/>
    <text x="${midX}" y="${midY + 140}" text-anchor="middle" font-family="Baskerville, Georgia, serif" font-size="24" letter-spacing="4" fill="${palette.accentGlow}" opacity="0.52">SPELLWAKE ATELIER</text>
  </g>`;
}

function renderDfcBadge(layout: CardLayoutKind, faceIndex: 0 | 1): string {
  if (layout !== "transform" && layout !== "modal_dfc") return "";
  const icon =
    layout === "transform"
      ? faceIndex === 0
        ? `<circle cx="106" cy="113" r="14" fill="#f3d68b" stroke="#22190b" stroke-width="2.4"/>`
        : `<path d="M 112 99 A 14 14 0 1 0 112 127 A 10.5 10.5 0 1 1 112 99 Z" fill="#d9e3f2" stroke="#151b26" stroke-width="2.2"/>`
      : faceIndex === 0
        ? `<polygon points="106,98 119,124 93,124" fill="#f3d68b" stroke="#1e170a" stroke-width="2.2"/>`
        : `<polygon points="106,126 119,100 93,100" fill="#d7e4f6" stroke="#141a24" stroke-width="2.2"/>`;
  return `<g>
    <circle cx="106" cy="113" r="24" fill="#181b21" stroke="#c8b07b" stroke-width="2.4"/>
    ${icon}
  </g>`;
}

function renderRulesBoxContent(
  face: CardFaceRenderData,
  boxX: number,
  boxY: number,
  boxW: number,
  boxH: number,
  ink: string,
): string {
  const padX = 28;
  const padY = 26;
  const innerW = boxW - padX * 2;
  const paragraphs = face.oracleText
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  const flavor = face.flavorText?.trim() ?? "";

  const totalChars = paragraphs.join(" ").length + flavor.length;
  let fontSize = 42;
  if (totalChars > 320) fontSize = 29;
  else if (totalChars > 220) fontSize = 33;
  else if (totalChars > 130) fontSize = 37;

  // Iteratively shrink if needed to avoid overflowing the rules box height.
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const lineHeight = fontSize * 1.24;
    const paraGap = fontSize * 0.45;
    let totalHeight = 0;
    const blocks: WrappedLine[][] = [];
    for (const p of paragraphs) {
      const wrapped = wrapParagraph(p, innerW, fontSize, false);
      blocks.push(wrapped);
      totalHeight += wrapped.length * lineHeight + paraGap;
    }
    const flavorLines = flavor ? wrapParagraph(flavor, innerW, fontSize * 0.92, true) : [];
    if (flavorLines.length > 0) {
      totalHeight += 24 + flavorLines.length * (fontSize * 0.92 * 1.22);
    }

    if (totalHeight <= boxH - padY * 2 || fontSize <= 18) {
      const parts: string[] = [];
      // Vertically center short rules text nicely, top-align dense text.
      let cursorY =
        totalHeight < (boxH - padY * 2) * 0.62
          ? boxY + (boxH - totalHeight) / 2 + fontSize * 0.85
          : boxY + padY + fontSize * 0.85;

      for (const block of blocks) {
        parts.push(
          renderWrappedRichLines(block, boxX + padX, cursorY, fontSize, lineHeight, ink),
        );
        cursorY += block.length * lineHeight + paraGap;
      }

      if (flavorLines.length > 0) {
        if (blocks.length > 0) {
          const ruleY = cursorY - paraGap * 0.35;
          parts.push(
            `<line x1="${boxX + padX + 24}" y1="${ruleY.toFixed(1)}" x2="${boxX + boxW - padX - 24}" y2="${ruleY.toFixed(1)}" stroke="${ink}" stroke-opacity="0.22" stroke-width="1.8"/>`,
          );
          cursorY += 18;
        }
        const fSize = fontSize * 0.92;
        parts.push(
          renderWrappedRichLines(
            flavorLines,
            boxX + padX,
            cursorY,
            fSize,
            fSize * 1.22,
            "#3a352f",
          ),
        );
      }
      return parts.join("");
    }
    fontSize -= 2.5;
  }
  return "";
}

function renderPlaneswalkerRules(
  face: CardFaceRenderData,
  boxX: number,
  boxY: number,
  boxW: number,
  boxH: number,
  palette: FramePalette,
): string {
  const lines = face.oracleText
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);
  if (lines.length === 0) return "";
  const rowH = Math.min(130, Math.floor(boxH / lines.length));
  const parts: string[] = [];
  lines.forEach((line, idx) => {
    const y = boxY + idx * rowH;
    const fill = idx % 2 === 0 ? palette.boxFill : palette.boxAltFill;
    parts.push(
      `<rect x="${boxX}" y="${y}" width="${boxW}" height="${rowH}" fill="${fill}" fill-opacity="0.93"/>`,
    );
    const loyaltyMatch = /^([+−-]?\d+|[+−-]?X|0):\s*(.*)$/i.exec(line);
    if (loyaltyMatch) {
      const badge = (loyaltyMatch[1] ?? "0").replace("-", "\u2212");
      const body = loyaltyMatch[2] ?? "";
      const badgeCy = y + rowH / 2;
      const isPlus = badge.startsWith("+");
      const isMinus = badge.startsWith("\u2212");
      const badgePath = isPlus
        ? `M ${boxX + 44} ${badgeCy - 30} L ${boxX + 88} ${badgeCy - 8} L ${boxX + 82} ${badgeCy + 28} L ${boxX + 6} ${badgeCy + 28} L ${boxX} ${badgeCy - 8} Z`
        : isMinus
          ? `M ${boxX + 6} ${badgeCy - 28} L ${boxX + 82} ${badgeCy - 28} L ${boxX + 88} ${badgeCy + 6} L ${boxX + 44} ${badgeCy + 30} L ${boxX} ${badgeCy + 6} Z`
          : `M ${boxX + 6} ${badgeCy - 24} L ${boxX + 82} ${badgeCy - 24} L ${boxX + 82} ${badgeCy + 24} L ${boxX + 6} ${badgeCy + 24} Z`;
      parts.push(`<path d="${badgePath}" fill="#181a1f" stroke="#d8caa8" stroke-width="3"/>`);
      parts.push(
        `<text x="${boxX + 44}" y="${(badgeCy + 10).toFixed(1)}" text-anchor="middle" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="29" fill="#f8f2e4">${escapeXml(badge)}</text>`,
      );
      const wrapped = wrapParagraph(body, boxW - 122, 26);
      parts.push(renderWrappedRichLines(wrapped, boxX + 104, y + 40, 26, 32, palette.boxInk));
    } else {
      const wrapped = wrapParagraph(line, boxW - 44, 26);
      parts.push(renderWrappedRichLines(wrapped, boxX + 22, y + 40, 26, 32, palette.boxInk));
    }
  });
  return parts.join("");
}

function renderSagaBody(
  face: CardFaceRenderData,
  palette: FramePalette,
  clipId: string,
): string {
  const leftX = 84;
  const topY = 168;
  const totalH = 1094;
  const leftW = 434;
  const rightX = leftX + leftW + 14;
  const rightW = 894 - leftW - 14;

  const lines = face.oracleText
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0);

  const chapterParts: string[] = [];
  chapterParts.push(
    `<rect x="${leftX}" y="${topY}" width="${leftW}" height="${totalH}" rx="10" fill="${palette.boxFill}" stroke="${palette.pinlineStart}" stroke-width="4"/>`,
  );
  chapterParts.push(
    `<text x="${leftX + 22}" y="${topY + 42}" font-family="Baskerville, Georgia, serif" font-style="italic" font-size="22" fill="#463e34">(As this Saga enters and after your draw step, add a lore counter.)</text>`,
  );

  const rowStart = topY + 86;
  const usableH = totalH - 102;
  const rowH = lines.length > 0 ? Math.min(260, Math.floor(usableH / lines.length)) : usableH;
  lines.forEach((line, idx) => {
    const y = rowStart + idx * rowH;
    const chapMatch = /^([IVX,\s]+)\s*[—–-]\s*(.*)$/.exec(line);
    const chapLabel = chapMatch
      ? (chapMatch[1] ?? "").trim()
      : (["I", "II", "III", "IV"][idx] ?? `${idx + 1}`);
    const body = chapMatch ? (chapMatch[2] ?? "").trim() : line;
    if (idx > 0) {
      chapterParts.push(
        `<line x1="${leftX + 18}" y1="${y}" x2="${leftX + leftW - 18}" y2="${y}" stroke="${palette.pinlineEnd}" stroke-opacity="0.45" stroke-width="2"/>`,
      );
    }
    const hexCy = y + 54;
    chapterParts.push(
      `<polygon points="${leftX + 42},${hexCy - 28} ${leftX + 68},${hexCy - 14} ${leftX + 68},${hexCy + 14} ${leftX + 42},${hexCy + 28} ${leftX + 16},${hexCy + 14} ${leftX + 16},${hexCy - 14}" fill="#24201a" stroke="#d8c08a" stroke-width="2.6"/>`,
    );
    chapterParts.push(
      `<text x="${leftX + 42}" y="${hexCy + 9}" text-anchor="middle" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="22" fill="#f7edd6">${escapeXml(chapLabel)}</text>`,
    );
    const wrapped = wrapParagraph(body, leftW - 100, 25);
    chapterParts.push(renderWrappedRichLines(wrapped, leftX + 84, y + 48, 25, 31, palette.boxInk));
  });

  const art = renderArtWindow(
    face.artUrl,
    rightX,
    topY,
    rightW,
    totalH,
    face.cropX ?? 0,
    face.cropY ?? 0,
    face.zoom ?? 1,
    palette,
    clipId,
  );
  return `${chapterParts.join("")}${art}<rect x="${rightX}" y="${topY}" width="${rightW}" height="${totalH}" fill="none" stroke="${palette.pinlineEnd}" stroke-width="4"/>`;
}

function renderCardFaceSvg(input: CardFaceFrameInput): string {
  const { face, layout, rarity } = input;
  const palette = resolveFramePalette(face, input.frameAccent ?? "auto");
  const prefix = input.idPrefix ?? "swcard";
  const clipId = `${prefix}-art-clip`;
  const frameGradId = `${prefix}-frame-grad`;
  const pinGradId = `${prefix}-pin-grad`;
  const faceIndex = input.faceIndex ?? 0;

  const isPlaneswalker = layout === "planeswalker";
  const isSaga = layout === "saga";
  const isAdventure = layout === "adventure" && Boolean(input.secondaryFace);
  const isSplit = layout === "split" && Boolean(input.secondaryFace);

  const manaBar = renderManaCostBar(face.manaCost, 962, 113, 42, 6);
  const hasDfcBadge = layout === "transform" || layout === "modal_dfc";
  const titleX = hasDfcBadge ? 142 : 102;
  const maxTitleW = Math.max(260, 956 - titleX - manaBar.totalWidth - 20);
  const titleSize = face.name.length * 23 > maxTitleW ? 36 : 44;

  const hasIndicator = Boolean(face.colorIndicator && face.colorIndicator.length > 0);
  const typeX = hasIndicator ? 142 : 102;
  const typeSize = face.typeLine.length > 36 ? 29 : 34;

  const artY = 168;
  const artH = isPlaneswalker ? 604 : 654;
  const typeY = isSaga ? 1272 : artY + artH + 12;
  const rulesY = typeY + 84;
  const rulesH = isPlaneswalker ? 492 : 434;

  const ptPresent = Boolean(face.power?.trim() || face.toughness?.trim());
  const loyaltyPresent = Boolean(face.loyalty?.trim());
  const defensePresent = Boolean(face.defense?.trim());

  let mainBodySvg = "";
  if (isSaga) {
    mainBodySvg = `
      ${renderSagaBody(face, palette, clipId)}
      <rect x="76" y="${typeY}" width="910" height="74" rx="18" fill="${palette.titleFill}" stroke="url(#${pinGradId})" stroke-width="5"/>
      ${hasIndicator ? renderColorIndicatorDot(face.colorIndicator, 114, typeY + 37, 17) : ""}
      <text x="${typeX}" y="${typeY + 49}" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="${typeSize}" fill="${palette.titleInk}">${escapeXml(face.typeLine)}</text>
      ${renderRarityEmblem(rarity, 946, typeY + 37)}
    `;
  } else if (isSplit && input.secondaryFace) {
    const left = face;
    const right = input.secondaryFace;
    const leftPal = resolveFramePalette(left, input.frameAccent ?? "auto");
    const rightPal = resolveFramePalette(right, "auto");
    mainBodySvg = `
      <g>
        <rect x="76" y="168" width="445" height="1182" rx="16" fill="${leftPal.frameTop}" stroke="${leftPal.pinlineStart}" stroke-width="5"/>
        <rect x="92" y="186" width="413" height="68" rx="14" fill="${leftPal.titleFill}" stroke="${leftPal.titleStroke}" stroke-width="3"/>
        <text x="108" y="231" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="30" fill="${leftPal.titleInk}">${escapeXml(left.name)}</text>
        ${renderManaCostBar(left.manaCost, 492, 220, 30, 4).svg}
        ${renderArtWindow(left.artUrl, 96, 266, 405, 420, left.cropX ?? 0, left.cropY ?? 0, left.zoom ?? 1, leftPal, `${clipId}-l`)}
        <rect x="92" y="698" width="413" height="58" rx="12" fill="${leftPal.titleFill}" stroke="${leftPal.titleStroke}" stroke-width="3"/>
        <text x="108" y="736" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="25" fill="${leftPal.titleInk}">${escapeXml(left.typeLine)}</text>
        <rect x="96" y="766" width="405" height="564" rx="8" fill="${leftPal.boxFill}" stroke="${leftPal.pinlineEnd}" stroke-width="3"/>
        ${renderRulesBoxContent(left, 96, 766, 405, 564, leftPal.boxInk)}
      </g>
      <g>
        <rect x="541" y="168" width="445" height="1182" rx="16" fill="${rightPal.frameTop}" stroke="${rightPal.pinlineStart}" stroke-width="5"/>
        <rect x="557" y="186" width="413" height="68" rx="14" fill="${rightPal.titleFill}" stroke="${rightPal.titleStroke}" stroke-width="3"/>
        <text x="573" y="231" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="30" fill="${rightPal.titleInk}">${escapeXml(right.name)}</text>
        ${renderManaCostBar(right.manaCost, 957, 220, 30, 4).svg}
        ${renderArtWindow(right.artUrl ?? left.artUrl, 561, 266, 405, 420, right.cropX ?? 0, right.cropY ?? 0, right.zoom ?? 1, rightPal, `${clipId}-r`)}
        <rect x="557" y="698" width="413" height="58" rx="12" fill="${rightPal.titleFill}" stroke="${rightPal.titleStroke}" stroke-width="3"/>
        <text x="573" y="736" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="25" fill="${rightPal.titleInk}">${escapeXml(right.typeLine)}</text>
        <rect x="561" y="766" width="405" height="564" rx="8" fill="${rightPal.boxFill}" stroke="${rightPal.pinlineEnd}" stroke-width="3"/>
        ${renderRulesBoxContent(right, 561, 766, 405, 564, rightPal.boxInk)}
      </g>
    `;
  } else {
    let rulesAreaSvg = "";
    if (isPlaneswalker) {
      rulesAreaSvg = `
        <rect x="84" y="${rulesY}" width="894" height="${rulesH}" rx="10" fill="${palette.boxFill}" stroke="url(#${pinGradId})" stroke-width="5"/>
        ${renderPlaneswalkerRules(face, 88, rulesY + 4, 886, rulesH - 8, palette)}
      `;
    } else if (isAdventure && input.secondaryFace) {
      const adv = input.secondaryFace;
      const advMana = renderManaCostBar(adv.manaCost, 514, rulesY + 34, 28, 4);
      rulesAreaSvg = `
        <rect x="84" y="${rulesY}" width="440" height="${rulesH}" rx="8" fill="${palette.boxAltFill}" stroke="url(#${pinGradId})" stroke-width="4.5"/>
        <rect x="88" y="${rulesY + 4}" width="432" height="58" fill="${palette.frameTop}" fill-opacity="0.92"/>
        <text x="104" y="${rulesY + 42}" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="27" fill="#f7f0e2">${escapeXml(adv.name)}</text>
        ${advMana.svg}
        <text x="104" y="${rulesY + 92}" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="23" fill="${palette.boxInk}">${escapeXml(adv.typeLine)}</text>
        <line x1="98" y1="${rulesY + 104}" x2="510" y2="${rulesY + 104}" stroke="${palette.pinlineEnd}" stroke-width="2"/>
        ${renderRulesBoxContent(adv, 88, rulesY + 108, 432, rulesH - 114, palette.boxInk)}
        <rect x="534" y="${rulesY}" width="444" height="${rulesH}" rx="8" fill="${palette.boxFill}" stroke="url(#${pinGradId})" stroke-width="4.5"/>
        ${renderRulesBoxContent(face, 534, rulesY, 444, rulesH, palette.boxInk)}
      `;
    } else {
      rulesAreaSvg = `
        <rect x="84" y="${rulesY}" width="894" height="${rulesH}" rx="8" fill="${palette.boxFill}" stroke="url(#${pinGradId})" stroke-width="5"/>
        ${renderRulesBoxContent(face, 84, rulesY, 894, rulesH, palette.boxInk)}
      `;
    }

    mainBodySvg = `
      ${renderArtWindow(face.artUrl, 84, artY, 894, artH, face.cropX ?? 0, face.cropY ?? 0, face.zoom ?? 1, palette, clipId)}
      <rect x="84" y="${artY}" width="894" height="${artH}" fill="none" stroke="url(#${pinGradId})" stroke-width="5"/>
      <rect x="76" y="${typeY}" width="910" height="74" rx="18" fill="${palette.titleFill}" stroke="url(#${pinGradId})" stroke-width="5"/>
      ${hasIndicator ? renderColorIndicatorDot(face.colorIndicator, 114, typeY + 37, 17) : ""}
      <text x="${typeX}" y="${typeY + 49}" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="${typeSize}" fill="${palette.titleInk}">${escapeXml(face.typeLine)}</text>
      ${renderRarityEmblem(rarity, 946, typeY + 37)}
      ${rulesAreaSvg}
    `;
  }

  let badgeSvg = "";
  if (ptPresent) {
    const ptText = `${face.power?.trim() || "0"}/${face.toughness?.trim() || "0"}`;
    badgeSvg = `<g>
      <rect x="790" y="1302" width="198" height="90" rx="18" fill="${palette.ptFill}" stroke="url(#${pinGradId})" stroke-width="5.5"/>
      <rect x="798" y="1310" width="182" height="74" rx="12" fill="none" stroke="#181512" stroke-opacity="0.24" stroke-width="2"/>
      <text x="889" y="1364" text-anchor="middle" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="50" fill="${palette.ptInk}">${escapeXml(ptText)}</text>
    </g>`;
  } else if (loyaltyPresent) {
    badgeSvg = `<g>
      <path d="M 834 1300 L 964 1300 L 976 1350 L 899 1404 L 822 1350 Z" fill="#17191f" stroke="#d8c394" stroke-width="5"/>
      <text x="899" y="1366" text-anchor="middle" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="48" fill="#f8f2e4">${escapeXml(face.loyalty!.trim())}</text>
    </g>`;
  } else if (defensePresent) {
    badgeSvg = `<g>
      <polygon points="899,1294 976,1322 958,1398 840,1398 822,1322" fill="#1c2028" stroke="#d8c394" stroke-width="5"/>
      <text x="899" y="1368" text-anchor="middle" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="46" fill="#f8f2e4">${escapeXml(face.defense!.trim())}</text>
    </g>`;
  }

  const holoStamp =
    rarity === "rare" || rarity === "mythic"
      ? `<g>
          <ellipse cx="531" cy="1364" rx="46" ry="20" fill="#14161b" stroke="#c8a85c" stroke-width="2.5"/>
          <ellipse cx="531" cy="1364" rx="36" ry="13" fill="url(#${pinGradId})" opacity="0.88"/>
        </g>`
      : "";

  const artistCredit = face.artist?.trim() ? `Illustrator: ${face.artist.trim()}` : "Spellwake Studio";
  const rarityInitial = (rarity[0] ?? "C").toUpperCase();
  const noteLine = input.collectorNote?.trim()
    ? `CUSTOM · ${rarityInitial} · ${input.collectorNote.trim()}`
    : `CUSTOM · ${rarityInitial} · OPEN PLAY`;

  const clipDefs = isSplit
    ? `<clipPath id="${clipId}-l"><rect x="96" y="266" width="405" height="420"/></clipPath>
       <clipPath id="${clipId}-r"><rect x="561" y="266" width="405" height="420"/></clipPath>`
    : isSaga
      ? `<clipPath id="${clipId}"><rect x="532" y="168" width="446" height="1094"/></clipPath>`
      : `<clipPath id="${clipId}"><rect x="84" y="${artY}" width="894" height="${artH}"/></clipPath>`;

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${CARD_WIDTH} ${CARD_HEIGHT}" width="${CARD_WIDTH}" height="${CARD_HEIGHT}" style="display:block;width:100%;height:100%">
  <defs>
    <linearGradient id="${frameGradId}" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="${palette.frameTop}"/>
      <stop offset="100%" stop-color="${palette.frameBottom}"/>
    </linearGradient>
    <linearGradient id="${pinGradId}" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="${palette.pinlineStart}"/>
      <stop offset="100%" stop-color="${palette.pinlineEnd}"/>
    </linearGradient>
    ${clipDefs}
  </defs>

  <!-- Outer Magic card black border -->
  <rect x="0" y="0" width="${CARD_WIDTH}" height="${CARD_HEIGHT}" rx="50" fill="#101114"/>
  <rect x="4" y="4" width="${CARD_WIDTH - 8}" height="${CARD_HEIGHT - 8}" rx="46" fill="none" stroke="#2c3039" stroke-width="2.5"/>

  <!-- Main card frame body -->
  <rect x="48" y="46" width="966" height="1320" rx="28" fill="url(#${frameGradId})" stroke="#0b0c0e" stroke-width="3"/>
  <rect x="58" y="56" width="946" height="1300" rx="22" fill="none" stroke="url(#${pinGradId})" stroke-opacity="0.55" stroke-width="2.5"/>

  ${
    !isSplit
      ? `<!-- Header Name + Mana Cost Bar -->
  <rect x="76" y="70" width="910" height="86" rx="20" fill="${palette.titleFill}" stroke="url(#${pinGradId})" stroke-width="5"/>
  ${renderDfcBadge(layout, faceIndex)}
  <text x="${titleX}" y="128" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="${titleSize}" fill="${palette.titleInk}">${escapeXml(face.name || "Untitled Card")}</text>
  ${manaBar.svg}`
      : `<!-- Split Card Top Banner -->
  <rect x="76" y="70" width="910" height="82" rx="18" fill="${palette.titleFill}" stroke="url(#${pinGradId})" stroke-width="5"/>
  <text x="531" y="124" text-anchor="middle" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="38" fill="${palette.titleInk}">${escapeXml(face.name)} // ${escapeXml(input.secondaryFace?.name ?? "")}</text>`
  }

  ${mainBodySvg}
  ${holoStamp}
  ${badgeSvg}

  <!-- Collector Footer -->
  <text x="78" y="1410" font-family="'Segoe UI', system-ui, sans-serif" font-size="22" font-weight="600" letter-spacing="1.2" fill="#d5cfc5">${escapeXml(noteLine)}</text>
  <text x="78" y="1442" font-family="Baskerville, Georgia, serif" font-size="23" fill="#b4ada2">${escapeXml(artistCredit)}</text>
  <text x="984" y="1442" text-anchor="end" font-family="'Segoe UI', system-ui, sans-serif" font-size="20" letter-spacing="1.4" fill="#8b8478">SPELLWAKE</text>
</svg>`;
}

/**
 * Render a unified 1062x1482 stack triggered/activated ability card sharing the exact
 * outer bevels, pinlines, and typography system of full custom cards.
 */
function renderStackTriggerSvg(input: StackTriggerFrameInput): string {
  const palette = PALETTES[input.frameAccent && input.frameAccent !== "auto" ? input.frameAccent : "gold"];
  const prefix = input.idPrefix ?? "swtrig";
  const clipId = `${prefix}-art-clip`;
  const frameGradId = `${prefix}-frame-grad`;
  const pinGradId = `${prefix}-pin-grad`;
  const trimmedEffect = input.effect.trim();
  const illustrated = Boolean(input.illustrated && input.artUrl && trimmedEffect.length <= 100);
  const titleSize =
    input.name.length > 32
      ? 38
      : input.name.length > 26
        ? 44
        : input.name.length > 20
          ? 50
          : 56;

  const typeY = illustrated ? 912 : 196;
  const rulesY = typeY + 104;
  const rulesH = 1408 - rulesY;

  const padX = 36;
  const padY = 34;
  const innerW = 934 - padX * 2;
  const paragraphs = trimmedEffect
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  let triggerFontSize = trimmedEffect.length <= 60 ? 74 : trimmedEffect.length <= 140 ? 66 : 62;
  let triggerTextSvg = "";
  for (let attempt = 0; attempt < 16; attempt += 1) {
    const lh = triggerFontSize * 1.24;
    const paraGap = triggerFontSize * 0.42;
    const blocks: WrappedLine[][] = [];
    let totalH = 0;
    for (let idx = 0; idx < paragraphs.length; idx += 1) {
      const wrapped = wrapParagraph(paragraphs[idx] ?? "", innerW, triggerFontSize, false);
      blocks.push(wrapped);
      totalH += wrapped.length * lh + (idx > 0 ? paraGap : 0);
    }
    if (totalH <= rulesH - padY * 2 || triggerFontSize <= 24) {
      let cursorY =
        illustrated && totalH < (rulesH - padY * 2) * 0.55
          ? rulesY + (rulesH - totalH) / 2 + triggerFontSize * 0.85
          : rulesY + padY + triggerFontSize * 0.88;
      const renderedBlocks: string[] = [];
      for (const block of blocks) {
        renderedBlocks.push(
          renderWrappedRichLines(
            block,
            64 + padX,
            cursorY,
            triggerFontSize,
            lh,
            "#f7f2e8",
          ),
        );
        cursorY += block.length * lh + paraGap;
      }
      triggerTextSvg = renderedBlocks.join("");
      break;
    }
    triggerFontSize -= 3;
  }

  let artSvg = "";
  if (illustrated) {
    if (input.artSourceKind === "full_card" && input.artUrl && input.artUrl.trim().length > 0) {
      // Crop a full 5:7 printed card face cleanly onto its upper artwork window.
      const fullW = 934 * 1.24;
      const fullH = fullW * 1.4;
      const fullX = 64 + (934 - fullW) / 2;
      const fullY = 182 - fullH * 0.116;
      artSvg = `<g clip-path="url(#${clipId})">
        <rect x="64" y="182" width="934" height="718" fill="#0d1117"/>
        <image href="${escapeXml(input.artUrl)}" x="${fullX.toFixed(1)}" y="${fullY.toFixed(1)}" width="${fullW.toFixed(1)}" height="${fullH.toFixed(1)}" preserveAspectRatio="xMidYMid slice"/>
      </g>
      <rect x="64" y="182" width="934" height="718" rx="12" fill="none" stroke="url(#${pinGradId})" stroke-width="4"/>`;
    } else {
      artSvg = `${renderArtWindow(input.artUrl, 64, 182, 934, 718, 0, 0, 1, palette, clipId)}
         <rect x="64" y="182" width="934" height="718" rx="12" fill="none" stroke="url(#${pinGradId})" stroke-width="4"/>`;
    }
  }

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${CARD_WIDTH} ${CARD_HEIGHT}" width="${CARD_WIDTH}" height="${CARD_HEIGHT}" style="display:block;width:100%;height:100%">
  <defs>
    <linearGradient id="${frameGradId}" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#222630"/>
      <stop offset="100%" stop-color="#11141a"/>
    </linearGradient>
    <linearGradient id="${pinGradId}" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="${palette.pinlineStart}"/>
      <stop offset="100%" stop-color="${palette.pinlineEnd}"/>
    </linearGradient>
    <clipPath id="${clipId}"><rect x="64" y="182" width="934" height="718" rx="12"/></clipPath>
  </defs>

  <rect x="0" y="0" width="${CARD_WIDTH}" height="${CARD_HEIGHT}" rx="50" fill="#0d0f13"/>
  <rect x="44" y="44" width="974" height="1394" rx="28" fill="url(#${frameGradId})" stroke="url(#${pinGradId})" stroke-width="5"/>

  <!-- Name Band -->
  <rect x="64" y="66" width="934" height="106" rx="18" fill="#181c24" stroke="url(#${pinGradId})" stroke-width="4.5"/>
  <text x="102" y="138" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="${titleSize}" fill="#faf5ec">${escapeXml(input.name)}</text>

  ${artSvg}

  <!-- Type Label Band -->
  <rect x="64" y="${typeY}" width="934" height="90" rx="16" fill="#1e2430" stroke="url(#${pinGradId})" stroke-width="4"/>
  <text x="102" y="${typeY + 60}" font-family="Baskerville, Georgia, serif" font-weight="700" font-size="46" fill="#f7f1e5">${escapeXml(input.typeLabel)}</text>

  <!-- Effect Rules Band -->
  <rect x="64" y="${rulesY}" width="934" height="${rulesH}" rx="14" fill="#141820" stroke="url(#${pinGradId})" stroke-opacity="0.78" stroke-width="4"/>
  ${triggerTextSvg}
</svg>`;
}

/** Generate a complete self-contained 1062x1482 SVG document for a card face or stack trigger. */
export function renderCardFrameSvg(input: CardFrameRenderInput): string {
  if (input.kind === "stack_trigger") {
    return renderStackTriggerSvg(input);
  }
  return renderCardFaceSvg(input);
}
