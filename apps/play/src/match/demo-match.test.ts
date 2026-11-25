import { describe, expect, test } from "bun:test";
import { applyDemoPlayLand } from "./demo-match";
import { PREVIEW_VIEWER, previewSnapshot } from "./preview-table";

describe("applyDemoPlayLand", () => {
  test("moves the land to the battlefield and decrements viewer handCount", () => {
    const before = previewSnapshot();
    const viewerBefore = before.players.find(
      (p) => p.seatId === PREVIEW_VIEWER,
    );
    const handBefore = before.zones.find((z) => z.handle === "zone-hand-you");
    expect(viewerBefore?.handCount).toBe("10");
    expect(handBefore?.visibility).toBe("visible");
    const mountain = before.visibleObjects.find(
      (o) =>
        o.zone === "zone-hand-you" &&
        o.identity.kind === "known" &&
        o.identity.characteristics.name === "Mountain",
    );
    expect(mountain).toBeDefined();

    const after = applyDemoPlayLand(before, mountain!.handle);
    const viewerAfter = after.players.find((p) => p.seatId === PREVIEW_VIEWER);
    const handAfter = after.zones.find((z) => z.handle === "zone-hand-you");
    const bfAfter = after.zones.find((z) => z.handle === "zone-battlefield");
    expect(viewerAfter?.handCount).toBe("9");
    expect(handAfter?.visibility === "visible" && handAfter.objects.length).toBe(
      9,
    );
    expect(
      bfAfter?.visibility === "visible" &&
        bfAfter.objects.includes(mountain!.handle),
    ).toBe(true);
  });
});
