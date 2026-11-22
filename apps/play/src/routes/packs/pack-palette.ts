import type { PackPrint } from "@spellwake/booster-pack";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * Wrapper colours. A set always gets the same one, from an FNV-1a hash of its
 * name; the order puts the approved walkthrough's four sets in its colours.
 * The 2D wrapper and the 3D opening print from the same palette.
 */
const PALETTES = [
  { hue: "#4f8a3c", deep: "#122612", glow: "#d8f0a080" },
  { hue: "#b07a2c", deep: "#2c1806", glow: "#ffd98a80" },
  { hue: "#7a4fb8", deep: "#21123f", glow: "#c9a8ff80" },
  { hue: "#a3343c", deep: "#2a0a10", glow: "#ff9a8a80" },
  { hue: "#2f8a86", deep: "#08221f", glow: "#9ef0e080" },
  { hue: "#a8456f", deep: "#2a0c1c", glow: "#ffb0d080" },
  { hue: "#355fae", deep: "#0d1838", glow: "#9cc0ff80" },
] as const;

export type WrapperPalette = (typeof PALETTES)[number];

export function paletteFor(name: string): WrapperPalette {
  let hash = 0x811c9dc5;
  for (let index = 0; index < name.length; index += 1) {
    hash ^= name.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return PALETTES[hash % PALETTES.length] ?? PALETTES[0];
}

/** The product kind under the set's name: "Foundations Play Booster" → "Play Booster". */
export function productKind(setName: string, product: string): string {
  return product.startsWith(setName)
    ? product.slice(setName.length).trim()
    : product;
}

/** Everything printed on a set's wrapper: its colours, name, kind and official symbol. */
export function packPrint(
  setName: string,
  product: string,
  symbolUrl: string | null,
): PackPrint {
  return {
    setName,
    kind: productKind(setName, product),
    palette: paletteFor(setName),
    symbolUrl,
    serif: shellTokens.serif,
  };
}
