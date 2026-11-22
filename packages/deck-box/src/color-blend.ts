/**
 * Copied from the deck-box asset study (`assets/deck-box/color-blend.ts`) so the package
 * holds all of its own code and type-checks without the private asset tree.
 * The study keeps its own copy for its previews. A change to one belongs in
 * the other.
 */
/** Normalized RGB triples describe the two source materials, without hue rotation. */
type Triple = readonly [number, number, number];

function rgb(hex: string): Triple {
  return [
    parseInt(hex.slice(1, 3), 16) / 255,
    parseInt(hex.slice(3, 5), 16) / 255,
    parseInt(hex.slice(5, 7), 16) / 255,
  ];
}

/** Preserve solid end colors with a broad, smooth blend between materials. */
export function makeFrameRamp(
  stops: readonly string[],
  samples = 1025,
): readonly Triple[] {
  const colors = stops.map(rgb);
  const halfWidth = 0.44 / Math.max(1, colors.length);
  return Array.from({ length: samples }, (_, index): Triple => {
    const position = index / (samples - 1);
    for (let segment = 0; segment < colors.length - 1; segment++) {
      const left = colors[segment],
        right = colors[segment + 1];
      if (!left || !right) continue;
      const boundary = (segment + 1) / colors.length;
      if (position <= boundary - halfWidth) return left;
      if (position < boundary + halfWidth) {
        const t = (position - boundary + halfWidth) / (2 * halfWidth);
        const mix = t * t * (3 - 2 * t);
        return [
          left[0] * (1 - mix) + right[0] * mix,
          left[1] * (1 - mix) + right[1] * mix,
          left[2] * (1 - mix) + right[2] * mix,
        ];
      }
    }
    return colors[colors.length - 1] ?? [0.5, 0.5, 0.5];
  });
}

/** Draw the same material ramp in the review strip, including its solid regions. */
export function frameGradient(ramp: readonly Triple[]): string {
  const stops = ramp
    .filter((_, index) => index % 16 === 0)
    .map(
      (color, index) =>
        `rgb(${color.map((channel) => Math.round(channel * 255)).join(" ")}) ${(index / Math.max(1, Math.ceil(ramp.length / 16) - 1)) * 100}%`,
    );
  return `linear-gradient(90deg in srgb,${stops.join(",")})`;
}
