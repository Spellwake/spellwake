import { expect, test } from "bun:test";
import { z } from "zod";
import {
  ownedPredictionMarketSchema,
  predictionMarketPageSchema,
  predictionOrderPageSchema,
  predictionPositionPageSchema,
  predictionDepthQuantitySchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
const f = z
  .strictObject({
    market: ownedPredictionMarketSchema,
    markets: predictionMarketPageSchema,
    orders: predictionOrderPageSchema,
    positions: predictionPositionPageSchema,
  })
  .parse(
    await Bun.file(
      new URL(
        "../../../../services/application/tests/fixtures/prediction-reads.json",
        import.meta.url,
      ),
    ).json(),
  );
const owner = f.market.accountId,
  market = f.market.market.terms.id;
const other = "99999999-9999-4999-8999-999999999999";
test("prediction projections preserve large anonymous depth, exact backing and submillisecond term order", () => {
  expect(f.market.market.book[0]?.quantity.length).toBe(79);
  expect(predictionDepthQuantitySchema.safeParse("9".repeat(83)).success).toBe(
    true,
  );
  expect(predictionDepthQuantitySchema.safeParse("9".repeat(84)).success).toBe(
    false,
  );
  expect(
    ownedPredictionMarketSchema.safeParse({
      ...f.market,
      market: {
        ...f.market.market,
        terms: {
          ...f.market.market.terms,
          tradingClosesAt: "2030-01-02T12:00:00.000001Z",
          observationStartsAt: "2030-01-02T12:00:00.000002Z",
        },
      },
    }).success,
  ).toBe(true);
  const missing = structuredClone(f.market);
  Reflect.deleteProperty(missing.market.terms, "outcome");
  expect(ownedPredictionMarketSchema.safeParse(missing).success).toBe(false);
  const firstOrder = f.orders.items[0],
    firstPosition = f.positions.items[0];
  if (!firstOrder || !firstPosition) throw new Error("Fixture required");
  for (const item of [
    { ...firstOrder, unfilledReservation: "119" },
    { ...firstOrder, actualFilledCost: "121" },
    { ...firstOrder, filledQuantity: "bad" },
  ])
    expect(
      predictionOrderPageSchema.safeParse({ ...f.orders, items: [item] })
        .success,
    ).toBe(false);
  expect(
    predictionPositionPageSchema.safeParse({
      ...f.positions,
      items: [{ ...firstPosition, fixedPayout: "101" }],
    }).success,
  ).toBe(false);
  expect(
    ownedPredictionMarketSchema.safeParse({
      ...f.market,
      market: {
        ...f.market.market,
        book: [...f.market.market.book, ...f.market.market.book],
      },
    }).success,
  ).toBe(false);
  expect(
    ownedPredictionMarketSchema.safeParse({
      ...f.market,
      market: { ...f.market.market, holdId: other },
    }).success,
  ).toBe(false);
});
test("prediction read client binds bearer, expected owner, resource and market filter", async () => {
  const seen: string[] = [];
  const api = ApplicationApi.create(
    "https://api.example",
    async (url, init) => {
      const u = new URL(String(url));
      seen.push(u.pathname + u.search);
      expect(new Headers(init?.headers).get("Authorization")).toBe(
        "Bearer original",
      );
      const data = u.pathname.endsWith("/orders")
        ? f.orders
        : u.pathname.endsWith("/positions")
          ? f.positions
          : u.pathname.endsWith("/markets")
            ? f.markets
            : f.market;
      return Response.json({ status: "success", data });
    },
  );
  expect((await api?.predictionMarket("original", owner, market))?.status).toBe(
    "success",
  );
  expect((await api?.predictionMarkets("original", owner))?.status).toBe(
    "success",
  );
  expect((await api?.predictionOrders("original", owner, market))?.status).toBe(
    "success",
  );
  expect(
    (await api?.predictionPositions("original", owner, market))?.status,
  ).toBe("success");
  expect(seen).toEqual([
    `/v1/predictions/markets/${market}`,
    "/v1/predictions/markets?limit=50",
    `/v1/predictions/orders?marketId=${market}&limit=50`,
    `/v1/predictions/positions?marketId=${market}&limit=50`,
  ]);
  expect((await api?.predictionMarket("original", other, market))?.status).toBe(
    "error",
  );
  expect((await api?.predictionOrders("original", owner, other))?.status).toBe(
    "error",
  );
  expect(
    (await api?.predictionPositions("original", owner, market, other))?.status,
  ).toBe("error");
});
