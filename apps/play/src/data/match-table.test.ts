import { describe, expect, test } from "bun:test";
import { playerSnapshotSchema } from "@spellwake/application-contracts";
import { PREVIEW_SEATS, previewSnapshot } from "../match/preview-table";
import {
  layoutTable,
  overviewCells,
  rowCentres,
  seatBoard,
} from "../match/table-model";

const snapshot = previewSnapshot();
const stage = { width: 1440, height: 900 };
const you = PREVIEW_SEATS[0].seatId;
const alex = PREVIEW_SEATS[1].seatId;

describe("the preview table", () => {
  test("is a snapshot the player-view contract accepts", () => {
    expect(playerSnapshotSchema.safeParse(snapshot).success).toBe(true);
  });

  test("each seat's board is read from the shared battlefield", () => {
    const board = seatBoard(snapshot, you);
    expect(board.front.map((card) => card.identity.kind)).toEqual([
      "known",
      "known",
      "known",
    ]);
    expect(board.lands).toHaveLength(4);
    expect(board.others).toHaveLength(1);
    expect(board.graveyard).toHaveLength(1);
    expect(board.hand).toHaveLength(7);
    // Another seat's hand is counted, never shown.
    const other = seatBoard(snapshot, alex);
    expect(other.hand).toHaveLength(0);
    expect(other.handCount).toBe(5);
  });
});

describe("laying out the table", () => {
  test("a row is centred, and squeezes rather than leaving its room", () => {
    expect(rowCentres([100, 100], 500, 1000, 20)).toEqual([440, 560]);
    const squeezed = rowCentres(
      Array.from({ length: 12 }, () => 100),
      500,
      600,
      10,
    );
    expect(squeezed[0]! - 50).toBeGreaterThanOrEqual(199.9);
    expect(squeezed.at(-1)! + 50).toBeLessThanOrEqual(800.1);
    expect(squeezed).toEqual([...squeezed].sort((a, b) => a - b));
  });

  test("your view shows your board and your hand, and no one else's cards", () => {
    const layout = layoutTable(snapshot, { kind: "seat", seatId: you }, stage);
    expect(layout.tilted).toBe(true);
    expect(new Set(layout.placements.map((p) => p.seatId))).toEqual(
      new Set([you]),
    );
    const hand = layout.placements.filter((p) => p.zone === "hand");
    expect(hand).toHaveLength(7);
    expect(hand.every((p) => p.kind === "card")).toBe(true);
    // Lands lie nearer the player than creatures do.
    const y = (kind: string) =>
      layout.placements.find((p) =>
        p.object?.publicProperties?.cardTypes.includes(kind as never),
      )!.y;
    expect(y("land")).toBeGreaterThan(y("creature"));
  });

  test("another seat's hand is shown as backs", () => {
    const layout = layoutTable(snapshot, { kind: "seat", seatId: alex }, stage);
    const hand = layout.placements.filter((p) => p.zone === "hand");
    expect(hand).toHaveLength(5);
    expect(hand.every((p) => p.kind === "back" && p.object === null)).toBe(
      true,
    );
    // A tapped land lies on its side.
    expect(
      layout.placements.filter((p) => p.rotate === 90 && p.zone !== "hand"),
    ).toHaveLength(2);
  });

  test("the whole table gives six seats fixed cells and no hands", () => {
    const layout = layoutTable(snapshot, { kind: "overview" }, stage);
    expect(layout.tilted).toBe(false);
    expect(layout.labels).toHaveLength(6);
    expect(layout.placements.some((p) => p.zone === "hand")).toBe(false);
    expect(overviewCells(6).filter((cell) => cell.row === "top")).toHaveLength(
      3,
    );
    // Every card stays on the stage.
    for (const p of layout.placements) {
      expect(p.x).toBeGreaterThan(0);
      expect(p.x).toBeLessThan(stage.width);
      expect(p.y).toBeGreaterThan(0);
      expect(p.y).toBeLessThan(stage.height);
    }
  });

  test("a card keeps one identity across views, so it travels", () => {
    const home = layoutTable(snapshot, { kind: "seat", seatId: you }, stage);
    const whole = layoutTable(snapshot, { kind: "overview" }, stage);
    const keys = new Set(whole.placements.map((p) => p.key));
    for (const p of home.placements.filter((p) => p.zone !== "hand"))
      expect(keys.has(p.key)).toBe(true);
  });

  test("docked controls push the board up, not off the stage", () => {
    const phone = { width: 390, height: 844 };
    const free = layoutTable(snapshot, { kind: "seat", seatId: you }, phone);
    const docked = layoutTable(
      snapshot,
      { kind: "seat", seatId: you },
      phone,
      230,
    );
    const lowest = (layout: typeof free) =>
      Math.max(
        ...layout.placements.filter((p) => p.zone !== "hand").map((p) => p.y),
      );
    expect(lowest(docked)).toBeLessThan(lowest(free));
    expect(lowest(docked)).toBeLessThan(phone.height - 230);
  });
});
