import { expect, test } from "bun:test";
import type { Purchase } from "@spellwake/application-contracts";
import {
  purchaseAmount,
  purchaseDate,
  purchaseRows,
} from "../account/purchase-rows";

const purchase = (over: Partial<Purchase>): Purchase =>
  ({
    purchaseId: "00000000-0000-4000-8000-000000000001",
    cosmeticId: "00000000-0000-4000-8000-0000000000aa",
    createdAt: "2026-11-01T23:30:00Z",
    state: "paid",
    revision: "1",
    price: { currency: "usd", minorUnits: "299" },
    checkout: { status: "unavailable" },
    ...over,
  }) as Purchase;

test("an amount is shown in whole units of its own currency", () => {
  expect(purchaseAmount({ currency: "usd", minorUnits: "299" })).toBe(
    "$2.99 USD",
  );
  expect(purchaseAmount({ currency: "usd", minorUnits: "300" })).toBe(
    "$3.00 USD",
  );
  // The yen has no smaller unit, so 500 is five hundred yen, not five.
  expect(purchaseAmount({ currency: "jpy", minorUnits: "500" })).toBe(
    "¥500 JPY",
  );
});

test("a currency that cannot be formatted is not given a guessed figure", () => {
  expect(purchaseAmount({ currency: "zz1", minorUnits: "299" })).toBeNull();
  expect(
    purchaseRows(
      [purchase({ price: { currency: "zz1", minorUnits: "299" } })],
      new Map(),
    )[0]?.amount,
  ).toBe("—");
});

test("rows carry the catalogue name, the day in UTC and the server's status", () => {
  const rows = purchaseRows(
    [
      purchase({}),
      purchase({
        purchaseId: "00000000-0000-4000-8000-000000000002",
        cosmeticId: "00000000-0000-4000-8000-0000000000bb",
        state: "reversed",
      }),
    ],
    new Map([["00000000-0000-4000-8000-0000000000aa", "Midnight sleeve"]]),
  );
  expect(rows.map((row) => [row.item, row.status])).toEqual([
    ["Midnight sleeve", "Paid"],
    ["Cosmetic", "Reversed"],
  ]);
  expect(purchaseDate(rows[0]?.date ?? "")).toBe("Nov 1, 2026");
  expect(purchaseDate("not a date")).toBe("—");
});
