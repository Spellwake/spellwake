/** Continuous folded fan: readable cards in the middle, overflow folded behind either end. */
import { compactFan, type FanPose } from "./compact-fan";

export type FoldedFanPose = FanPose & { depth: number; folded: boolean };

/** Focus changes spacing continuously without removing entries or changing their order. */
export function foldedFan(count: number, focus: number): FoldedFanPose[] {
  if (count <= 7)
    return compactFan(count, Math.max(0, count - 1) * 1.1).map((pose, i) => ({
      ...pose,
      depth: i,
      folded: false,
    }));
  const center = Math.min(count - 4, Math.max(3, focus));
  const first = Math.max(0, Math.ceil(center - 3));
  const last = Math.min(count - 1, Math.floor(center + 3));
  const bank = Math.max(first, count - last - 1);
  return Array.from({ length: count }, (_, i) => {
    const offset = i - center;
    const excess = Math.max(0, Math.abs(offset) - 3);
    const fold = 1 - Math.exp(-excess / 4);
    const side = Math.sign(offset);
    const x = Math.max(-3, Math.min(3, offset)) * 1.1 + side * 0.85 * fold;
    return {
      x,
      y: -x * x * 0.035 + fold * 0.12,
      angle: -Math.atan(x * 0.07) - side * fold * 0.3,
      depth: i < first ? i : i > last ? count - 1 - i : bank + i - first,
      folded: excess > 0,
    };
  });
}
