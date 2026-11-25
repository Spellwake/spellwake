import { z } from "zod";

/** Dragon Shield website swatches, used as display-color references.
 * https://www.dragonshield.com/products/crimson-matte-sleeves (2026-11-01).
 * These are not measured physical material albedos.
 */
const sleeveColors = [
  { name: "black", label: "Black", pigment: 0x101010 },
  { name: "red", label: "Blood Red", pigment: 0x893840 },
  { name: "blue", label: "Midnight Blue", pigment: 0x162446 },
  { name: "orange", label: "Orange", pigment: 0xe38c25 },
  { name: "pink", label: "Pink Sapphire", pigment: 0xb82371 },
  { name: "green", label: "Forest Green", pigment: 0x527150 },
] as const;

export const solidSleeveDesigns = sleeveColors.flatMap((color) =>
  (["matte", "gloss"] as const).map((finish) => ({
    id: `${color.name}-${finish}`,
    label: `${color.label} · ${finish === "matte" ? "Matte" : "Glossy"}`,
    pigment: color.pigment,
    finish,
  })),
);

export const solidSleeveDesignSchema = z.enum(
  solidSleeveDesigns.map((design) => design.id),
);
