import { describe, expect, test } from "bun:test";
import { otherParty, targetOf, toMarkRead, updates } from "./updates";

const id = (n: number) =>
  `${String(n).padStart(8, "0")}-0000-4000-8000-000000000000`;
const item = (
  n: number,
  sourceKind: string,
  body: string,
  readAt: string | null = null,
) => ({ id: id(n), sourceKind, sourceRef: id(100 + n), body, readAt });

describe("the updates shown", () => {
  const list = updates([
    item(1, "trade_accepted", "Trade accepted", "2026-11-24T10:00:00Z"),
    item(2, "trade_offer", "Trade offer received"),
    { id: "broken" },
    item(3, "market_sold", "Marketplace listing sold"),
    item(4, "room_invite", "You were invited to a table"),
  ]);

  test("are those the contract allows, unread first, in the server's order", () => {
    expect(list.map((update) => [update.kind, update.isNew])).toEqual([
      ["trade-offer", true],
      ["sold", true],
      ["other", true],
      ["trade-accepted", false],
    ]);
    expect(list[0]?.text).toBe("Trade offer received");
  });

  test("mark only the unread ones read, a hundred at most", () => {
    expect(toMarkRead(list)).toEqual([id(2), id(3), id(4)]);
    const many = updates(
      Array.from({ length: 130 }, (_, n) => item(n + 10, "trade_offer", "x")),
    );
    expect(toMarkRead(many)).toHaveLength(100);
  });

  test("lead to the trade or to your listings, and a kind the app doesn't know leads nowhere", () => {
    expect(targetOf(list[0]!)).toEqual({ to: "trade", tradeId: id(102) });
    expect(targetOf(list[1]!)).toEqual({ to: "listings" });
    expect(targetOf(list[2]!)).toBeNull();
  });
});

describe("the other person in a trade", () => {
  const trade = { sender: { id: id(50) }, recipient: { id: id(51) } };
  test("is whoever the viewer is not", () => {
    expect(otherParty(trade, id(50))).toBe(id(51));
    expect(otherParty(trade, id(51))).toBe(id(50));
  });
});
