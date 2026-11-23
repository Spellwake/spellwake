/**
 * Where each printed mode sits on a card's original face, so a mode is chosen
 * on the card itself (MC-01). The game's mode numbers map to these regions by
 * printing; no rules text is parsed to find them. Coordinates are in pixels
 * of the face at the size given.
 */
export interface PrintedModeRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface PrintedModes {
  /** The face these regions were measured on. */
  width: number;
  height: number;
  /** One region per mode, in printed order. */
  regions: readonly PrintedModeRegion[];
}

const paragraph = (y: number, bottom: number): PrintedModeRegion => ({
  x: 37,
  y,
  width: 414,
  height: bottom - y,
});

/** Keyed by set code and collector number, as the catalogue records a printing. */
const BY_PRINTING: Record<string, PrintedModes> = {
  // Abrade: 3 damage to target creature; or destroy target artifact.
  "TRC/449": {
    width: 488,
    height: 680,
    regions: [paragraph(449, 497), paragraph(497, 526)],
  },
};

/** The mode regions of a printing, or null when none have been measured. */
export function printedModes(
  printing: { set: string; number: string } | null | undefined,
): PrintedModes | null {
  return printing
    ? (BY_PRINTING[`${printing.set.toUpperCase()}/${printing.number}`] ?? null)
    : null;
}
