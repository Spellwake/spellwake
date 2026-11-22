/**
 * The 3D booster pack and its opening. Three.js stays behind a dynamic
 * import, so screens that only list packs never load it.
 */
export type { PackStage, PackStageOptions, Rect } from "./pack-stage";
export type { PackPalette, PackPrint } from "./pack-print";
export type { Curve, MotionBeats, MotionCurves } from "./pack-timeline";

export async function mountPackStage(
  ...parameters: Parameters<typeof import("./pack-stage").mountPackStage>
): ReturnType<typeof import("./pack-stage").mountPackStage> {
  const stage = await import("./pack-stage");
  return stage.mountPackStage(...parameters);
}

/** Fetch the stage ahead of time, so an opening does not wait for it. */
export function preloadPackStage(): void {
  void import("./pack-stage").catch(() => undefined);
}

/** A still of a sealed pack, from the same model as the opening, to draw into a canvas. */
export async function packStill(
  ...parameters: Parameters<typeof import("./pack-still").packStill>
): ReturnType<typeof import("./pack-still").packStill> {
  const still = await import("./pack-still");
  return still.packStill(...parameters);
}
