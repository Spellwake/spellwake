import { describe, expect, test } from "bun:test";
import { beforeTilt, TILT_DEGREES, TILT_DISTANCE } from "./table-model";

/** Where a line drawn `drawn` pixels above the lower edge appears once the board leans. */
function afterTilt(drawn: number): number {
  const angle = (TILT_DEGREES * Math.PI) / 180;
  return (
    (drawn * Math.cos(angle) * TILT_DISTANCE) /
    (TILT_DISTANCE + drawn * Math.sin(angle))
  );
}

describe("the leaning board", () => {
  test("a line drawn where beforeTilt says appears where it was asked for", () => {
    for (const shown of [0, 54, 216, 480, 700])
      expect(afterTilt(beforeTilt(shown))).toBeCloseTo(shown, 6);
  });
  test("a higher line must be lifted by more than a lower one", () => {
    expect(beforeTilt(500) - 500).toBeGreaterThan(beforeTilt(216) - 216);
    expect(beforeTilt(216)).toBeGreaterThan(216);
  });
});
