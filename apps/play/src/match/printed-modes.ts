/**
 * Where each printed mode, or each printed instruction that takes a target,
 * sits on a card's original face, so it is chosen and followed on the card
 * itself (MC-01, MM-01, MT-01). The game's mode and target numbers map to
 * these regions by printing; no rules text is parsed to find them.
 * Coordinates are in pixels of the face at the size given.
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
  /** One region per mode or targeting instruction, in printed order. None for a card followed as a whole. */
  regions: readonly PrintedModeRegion[];
}

const paragraph = (
  y: number,
  bottom: number,
  x = 37,
  width = 414,
): PrintedModeRegion => ({ x, y, width, height: bottom - y });

/** Keyed by set code and collector number, as the catalogue records a printing. */
const BY_PRINTING: Record<string, PrintedModes> = {
  // Abrade: 3 damage to target creature; or destroy target artifact.
  "TRC/449": {
    width: 488,
    height: 680,
    regions: [paragraph(449, 497), paragraph(497, 526)],
  },
  // Cryptic Command: counter; return; tap; draw.
  "TRC/435": {
    width: 488,
    height: 680,
    regions: [
      paragraph(459, 485),
      paragraph(486, 537),
      paragraph(538, 589),
      paragraph(590, 618),
    ],
  },
  // Common Bond: its two identical instructions, first then second.
  "RTR/151": {
    width: 488,
    height: 680,
    regions: [paragraph(449, 478, 44, 402), paragraph(479, 508, 44, 402)],
  },
  // Arc Lightning: one instruction, followed on the whole card.
  "KTK/97": { width: 488, height: 680, regions: [] },
};

/** The mode regions of a printing, or null when none have been measured. */
export function printedModes(
  printing: { set: string; number: string } | null | undefined,
): PrintedModes | null {
  return printing
    ? (BY_PRINTING[`${printing.set.toUpperCase()}/${printing.number}`] ?? null)
    : null;
}
