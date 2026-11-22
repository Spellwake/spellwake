/** Shared native/Zod fixtures and untrusted caller-response boundaries. */
import { expect, test } from "bun:test";
import { z } from "zod";
import {
  ownedTradeSchema,
  ownedListingSchema,
  settlementSummarySchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";

const source: unknown = await Bun.file(
  new URL(
    "../../../../services/application/tests/fixtures/exchange-reads.json",
    import.meta.url,
  ),
).json();
const f = z
  .strictObject({
    offeredTrade: ownedTradeSchema,
    acceptedTrade: ownedTradeSchema,
    listedListing: ownedListingSchema,
    soldListing: ownedListingSchema,
  })
  .parse(source);
const owner = f.offeredTrade.accountId;
const trade = f.offeredTrade.trade.id;
const listing = f.listedListing.listing.id;
const outsider = "99999999-9999-4999-8999-999999999999";

test("exchange wire records preserve anonymous history and reject private or inconsistent facts", () => {
  expect(
    ownedTradeSchema.safeParse({
      ...f.offeredTrade,
      trade: {
        ...f.offeredTrade.trade,
        recipient: {
          ...f.offeredTrade.trade.recipient,
          username: "Deleted player",
          isDeleted: true,
        },
      },
    }).success,
  ).toBe(true);
  for (const bad of [
    { ...f.offeredTrade, accountId: outsider },
    {
      ...f.offeredTrade,
      trade: { ...f.offeredTrade.trade, senderHoldId: outsider },
    },
    {
      ...f.offeredTrade,
      trade: {
        ...f.offeredTrade.trade,
        recipient: { ...f.offeredTrade.trade.recipient, isDeleted: true },
      },
    },
    {
      ...f.acceptedTrade,
      trade: { ...f.acceptedTrade.trade, settlement: null },
    },
    {
      ...f.offeredTrade,
      trade: {
        ...f.offeredTrade.trade,
        give: [...f.offeredTrade.trade.give, ...f.offeredTrade.trade.give],
      },
    },
  ])
    expect(ownedTradeSchema.safeParse(bad).success).toBe(false);
  for (const bad of [
    { ...f.soldListing, accountId: outsider },
    {
      ...f.listedListing,
      listing: { ...f.listedListing.listing, participant: undefined },
    },
    {
      ...f.listedListing,
      listing: {
        ...f.listedListing.listing,
        assessment: { secret: "unavailable" },
      },
    },
    {
      ...f.soldListing,
      listing: {
        ...f.soldListing.listing,
        participant: { ...f.soldListing.listing.participant, paidCoins: "1" },
      },
    },
    {
      ...f.soldListing,
      listing: {
        ...f.soldListing.listing,
        participant: { ...f.soldListing.listing.participant, buyer: null },
      },
    },
  ])
    expect(ownedListingSchema.safeParse(bad).success).toBe(false);
  const summary = f.acceptedTrade.trade.settlement;
  if (!summary) throw new Error("Expected settlement fixture");
  for (const fraction of [
    { numerator: "invalid", denominator: "1" },
    { numerator: "1", denominator: "0" },
    { numerator: "200", denominator: "2" },
  ]) {
    expect(
      settlementSummarySchema.safeParse({
        ...summary,
        outgoing: { ...summary.outgoing, total: fraction },
      }).success,
    ).toBe(false);
  }
  const large = {
    numerator: `1${"0".repeat(8190)}1`,
    denominator: `1${"0".repeat(8191)}`,
  };
  expect(
    settlementSummarySchema.safeParse({
      ...summary,
      outgoing: { ...summary.outgoing, total: large },
      incoming: { ...summary.incoming, total: large },
    }).success,
  ).toBe(true);
});

test("exchange caller reads carry only the original bearer and exact record or page filters", async () => {
  const seen: { url: string; bearer: string | null; body: unknown }[] = [];
  let response: unknown;
  const api = ApplicationApi.create(
    "https://api.example",
    async (input, options) => {
      seen.push({
        url: String(input),
        bearer: new Headers(options?.headers).get("authorization"),
        body: options?.body ?? null,
      });
      return new Response(
        JSON.stringify({ status: "success", data: response }),
      );
    },
  );
  if (!api) throw new Error("Expected client");
  response = f.acceptedTrade;
  expect((await api.trade("original", owner, trade)).status).toBe("success");
  response = f.soldListing;
  expect((await api.marketListing("original", owner, listing)).status).toBe(
    "success",
  );
  response = {
    accountId: owner,
    items: [f.offeredTrade.trade],
    nextCursor: trade,
  };
  expect(
    (await api.trades("original", owner, { limit: 1, state: "offered" }))
      .status,
  ).toBe("success");
  response = {
    accountId: owner,
    items: [f.soldListing.listing],
    nextCursor: null,
  };
  expect(
    (await api.marketListings("original", owner, { owned: true })).status,
  ).toBe("success");
  response = {
    accountId: owner,
    items: [f.listedListing.listing],
    nextCursor: null,
  };
  expect((await api.marketListings("original", owner)).status).toBe("success");
  expect(seen).toEqual(
    [
      `/v1/trades/${trade}`,
      `/v1/market/listings/${listing}`,
      "/v1/trades?state=offered&limit=1",
      "/v1/market/listings?owned=true&limit=8",
      "/v1/market/listings?owned=false&limit=8",
    ].map((path) => ({
      url: `https://api.example${path}`,
      bearer: "Bearer original",
      body: null,
    })),
  );
});

test("exchange delivery rejects mismatched owners, replayed or oversized pages and wrong history scope", async () => {
  let response: unknown;
  let calls = 0;
  const api = ApplicationApi.create("https://api.example", async () => {
    calls++;
    return new Response(JSON.stringify({ status: "success", data: response }));
  });
  if (!api) throw new Error("Expected client");
  response = f.offeredTrade;
  expect((await api.trade("original", outsider, trade)).status).toBe("error");
  expect((await api.trade("original", owner, outsider)).status).toBe("error");
  response = {
    accountId: owner,
    items: [f.offeredTrade.trade],
    nextCursor: trade,
  };
  expect(
    (await api.trades("original", owner, { after: trade, limit: 1 })).status,
  ).toBe("error");
  expect(
    (await api.trades("original", owner, { limit: 1, state: "accepted" }))
      .status,
  ).toBe("error");
  response = {
    accountId: owner,
    items: [f.offeredTrade.trade, { ...f.offeredTrade.trade, id: outsider }],
    nextCursor: outsider,
  };
  expect((await api.trades("original", owner, { limit: 1 })).status).toBe(
    "error",
  );
  response = {
    accountId: owner,
    items: [f.soldListing.listing],
    nextCursor: null,
  };
  expect((await api.marketListings("original", owner)).status).toBe("error");
  response = {
    accountId: owner,
    items: [f.listedListing.listing],
    nextCursor: null,
  };
  expect(
    (await api.marketListings("original", owner, { owned: true })).status,
  ).toBe("error");
  const before = calls;
  const injected = { limit: 1, actorId: outsider };
  expect((await api.trades("original", owner, injected)).status).toBe("fail");
  expect((await api.trades("original", owner, { limit: 9 })).status).toBe(
    "fail",
  );
  expect(calls).toBe(before);
});
