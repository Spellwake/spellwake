import { describe, expect, test } from "bun:test";
import {
  canMarkReady,
  capQuantity,
  COINS_ID,
  inTrayOrder,
  shortfalls,
  statusWords,
  withQuantity,
  type OfferLine,
  type TradeAsset,
  type TradeHolding,
} from "../trade/trade-sources";

const asset = (
  id: string,
  kind: TradeAsset["kind"] = "card",
  name = id,
): TradeAsset => ({
  id,
  kind,
  name,
  detail: null,
  artUrl: null,
  setName: null,
});
const line = (
  id: string,
  quantity: number,
  kind: TradeAsset["kind"] = "card",
  name = id,
): OfferLine => ({ ...asset(id, kind, name), quantity });
const holding = (
  id: string,
  owned: number,
  kind: TradeAsset["kind"] = "card",
): TradeHolding => ({ ...asset(id, kind), owned });

describe("trade quantities", () => {
  test("a typed or stepped amount is capped at what is owned, and never below zero", () => {
    expect(capQuantity(7, 4)).toBe(4);
    expect(capQuantity(-2, 4)).toBe(0);
    expect(capQuantity(2.9, 4)).toBe(2);
    expect(capQuantity(3, -1)).toBe(0);
  });

  test("setting zero removes a line; a new line joins at the end", () => {
    const lines = [line("bolt", 2), line("guide", 1)];
    expect(withQuantity(lines, asset("bolt"), 0).map((l) => l.id)).toEqual([
      "guide",
    ]);
    expect(withQuantity(lines, asset("shock"), 1).map((l) => l.id)).toEqual([
      "bolt",
      "guide",
      "shock",
    ]);
    expect(withQuantity(lines, asset("guide"), 3)[1]?.quantity).toBe(3);
  });

  test("coins sit after every card and pack", () => {
    const ordered = inTrayOrder([
      line(COINS_ID, 250, "coin"),
      line("bolt", 2),
      line("fdn", 1, "pack"),
    ]);
    expect(ordered.map((l) => l.id)).toEqual(["bolt", "fdn", COINS_ID]);
  });
});

describe("readiness", () => {
  const holdings = [holding("bolt", 2), holding(COINS_ID, 1000, "coin")];

  test("a gift is allowed, an empty trade is not", () => {
    expect(canMarkReady([], [line("krenko", 1)], holdings)).toBe(true);
    expect(canMarkReady([line("bolt", 1)], [], holdings)).toBe(true);
    expect(canMarkReady([], [], holdings)).toBe(false);
  });

  test("a line above what is owned blocks Ready and says why", () => {
    const own = [
      line("bolt", 3, "card", "Lightning Bolt"),
      line(COINS_ID, 1500, "coin", "Coins"),
    ];
    expect(canMarkReady(own, [], holdings)).toBe(false);
    expect(shortfalls(own, holdings).map((s) => s.reason)).toEqual([
      "Only 2 Lightning Bolt available.",
      "1,000 coins available.",
    ]);
    expect(
      shortfalls([line("gone", 1, "card", "Shock")], holdings)[0]?.reason,
    ).toBe("Shock is no longer available.");
  });

  test("the status line names the other person", () => {
    expect(statusWords("editing", "ready", "Mira")).toBeNull();
    expect(statusWords("ready", "editing", "Mira")).toBe("Waiting for Mira");
    expect(statusWords("ready", "ready", "Mira")).toBe("Both ready");
    expect(statusWords("confirmed", "ready", "Mira")).toBe(
      "Waiting for Mira to confirm",
    );
  });
});
