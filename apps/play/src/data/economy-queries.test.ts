import { expect, test } from "bun:test";
import { QueryClient } from "@tanstack/react-query";
import type { Session } from "@supabase/supabase-js";
import { z } from "zod";
import {
  ownedTradeSchema,
  ownedListingSchema,
} from "@spellwake/application-contracts";
import { ApplicationApi } from "./application-api";
import {
  packProductsQueryOptions,
  packQuoteQueryOptions,
  inventoryQueryOptions,
  tradesQueryOptions,
  marketQueryOptions,
  tradeQueryOptions,
  listingQueryOptions,
  packOutcomeQueryOptions,
  type EconomyReadScope,
} from "./economy-queries";
import { ownsDeletionQuery } from "./deletion-recovery";
import { refreshEconomyQueries } from "./economy-cache";

const f = z
  .strictObject({
    offeredTrade: ownedTradeSchema,
    acceptedTrade: ownedTradeSchema,
    listedListing: ownedListingSchema,
    soldListing: ownedListingSchema,
  })
  .parse(
    await Bun.file(
      new URL(
        "../../../../services/application/tests/fixtures/exchange-reads.json",
        import.meta.url,
      ),
    ).json(),
  );
const origin = "https://api.example";
const subject = "22222222-2222-4222-8222-222222222222";
const sessionId = "33333333-3333-4333-8333-333333333333";
const opening = "44444444-4444-4444-8444-444444444444";
function scope(
  fetcher: NonNullable<Parameters<typeof ApplicationApi.create>[1]>,
  isCurrent = () => true,
): EconomyReadScope {
  const api = ApplicationApi.create(origin, fetcher);
  if (!api) throw new Error("Expected configured API");
  const session: Session = {
    access_token: "original-bearer",
    refresh_token: "unit-refresh",
    token_type: "bearer",
    expires_in: 3600,
    user: {
      id: subject,
      aud: "authenticated",
      app_metadata: {},
      user_metadata: {},
      created_at: "2026-11-02T00:00:00Z",
    },
  };
  return {
    owner: {
      accountId: f.offeredTrade.accountId,
      subjectId: subject,
      sessionId,
      state: "active",
      method: "email_code",
    },
    services: {
      api,
      auth: { getSession: async () => ({ data: { session }, error: null }) },
    },
    isCurrent,
  };
}
function client() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

test("economy queries consume original bearer, exact cursor and committed detail projections", async () => {
  const seen: string[] = [];
  const trades = Array.from({ length: 8 }, (_, index) => ({
    ...f.offeredTrade.trade,
    id: `000000${String(index + 10).padStart(2, "0")}-1111-4111-8111-111111111111`,
  }));
  const source = scope(async (url, init) => {
    const path = new URL(String(url));
    seen.push(path.pathname + path.search);
    expect(new Headers(init?.headers).get("Authorization")).toBe(
      "Bearer original-bearer",
    );
    let data: unknown;
    if (path.pathname === "/v1/trades") {
      expect(path.searchParams.get("state")).toBe("offered");
      data = {
        accountId: source.owner.accountId,
        items: path.searchParams.has("after") ? [] : trades,
        nextCursor: path.searchParams.has("after") ? null : trades.at(-1)?.id,
      };
    } else if (path.pathname === "/v1/market/listings")
      data = {
        accountId: source.owner.accountId,
        items: [f.listedListing.listing],
        nextCursor: null,
      };
    else if (path.pathname === "/v1/inventory")
      data = { items: [], nextCursor: null };
    else if (path.pathname.startsWith("/v1/packs/openings/"))
      data = {
        accountId: source.owner.accountId,
        outcome: {
          id: opening,
          inventoryAssetId: sessionId,
          recipeId: subject,
          operationId: opening,
          contents: [{ printingId: subject, quantity: "1" }],
        },
      };
    else if (path.pathname.startsWith("/v1/trades/")) data = f.offeredTrade;
    else data = f.listedListing;
    return Response.json({ status: "success", data });
  });
  const queries = client();
  try {
    const pages = await queries.fetchInfiniteQuery({
      ...tradesQueryOptions(source, { state: "offered" }),
      pages: 2,
    });
    expect(pages.pages.map((page) => page.items.length)).toEqual([8, 0]);
    expect(seen[1]).toContain(`after=${trades.at(-1)?.id}`);
    expect(
      (await queries.fetchInfiniteQuery(inventoryQueryOptions(source))).pages[0]
        ?.items,
    ).toEqual([]);
    expect(
      (await queries.fetchInfiniteQuery(marketQueryOptions(source))).pages[0]
        ?.items,
    ).toEqual([f.listedListing.listing]);
    expect(
      (
        await queries.fetchQuery(
          tradeQueryOptions(source, f.offeredTrade.trade.id),
        )
      ).trade,
    ).toEqual(f.offeredTrade.trade);
    expect(
      (
        await queries.fetchQuery(
          listingQueryOptions(source, f.listedListing.listing.id),
        )
      ).listing,
    ).toEqual(f.listedListing.listing);
    expect(
      (await queries.fetchQuery(packOutcomeQueryOptions(source, opening)))
        .outcome.id,
    ).toBe(opening);
    expect(seen).toHaveLength(7);
  } finally {
    queries.clear();
  }
});

test("session replacement and cancellation discard delayed economic responses without filling caches", async () => {
  for (const mode of ["replacement", "cancel"] as const) {
    let current = true;
    let finish: (() => void) | undefined;
    let started: (() => void) | undefined;
    const entered = new Promise<void>((resolve) => {
      started = resolve;
    });
    const delayed = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const source = scope(
      async () => {
        started?.();
        await delayed;
        return Response.json({ status: "success", data: f.offeredTrade });
      },
      () => current,
    );
    const queries = client();
    const options = tradeQueryOptions(source, f.offeredTrade.trade.id);
    const result = queries.fetchQuery(options);
    const observation = result.then(
      () => ({ rejected: false }),
      () => ({ rejected: true }),
    );
    await entered;
    if (mode === "cancel")
      await queries.cancelQueries({ queryKey: options.queryKey });
    else current = false;
    finish?.();
    expect((await observation).rejected).toBe(true);
    expect(queries.getQueryData(options.queryKey)).toBeUndefined();
    queries.clear();
  }
});

test("economic cache identity and terminal cleanup isolate account, API, session and filters", async () => {
  const source = scope(async () => {
    throw new Error("No request expected");
  });
  const queries = client();
  const options = [
    packProductsQueryOptions(source),
    packQuoteQueryOptions(source, opening),
    inventoryQueryOptions(source),
    tradesQueryOptions(source),
    marketQueryOptions(source),
    tradeQueryOptions(source, f.offeredTrade.trade.id),
    listingQueryOptions(source, f.listedListing.listing.id),
    packOutcomeQueryOptions(source, opening),
  ];
  const originalKeys: (readonly unknown[])[] = options.map(
    (option) => option.queryKey,
  );
  for (const key of originalKeys)
    queries.setQueryData(key, { private: "owned" });
  const foreignKeys = originalKeys.flatMap((key) => [
    [key[0], subject, ...key.slice(2)],
    [key[0], key[1], "https://other.example", ...key.slice(3)],
  ]);
  for (const key of foreignKeys)
    queries.setQueryData(key, { private: "foreign" });
  expect(
    tradesQueryOptions(source, { state: "accepted" }).queryKey,
  ).not.toEqual(tradesQueryOptions(source, { state: "offered" }).queryKey);
  expect(marketQueryOptions(source, { owned: true }).queryKey).not.toEqual(
    marketQueryOptions(source, { owned: false }).queryKey,
  );
  expect(
    tradeQueryOptions(
      { ...source, owner: { ...source.owner, sessionId: opening } },
      f.offeredTrade.trade.id,
    ).queryKey,
  ).not.toEqual(tradeQueryOptions(source, f.offeredTrade.trade.id).queryKey);
  await refreshEconomyQueries(queries, source.owner.accountId, origin);
  for (const key of originalKeys)
    expect(queries.getQueryState(key)?.isInvalidated).toBe(true);
  for (const key of foreignKeys)
    expect(queries.getQueryState(key)?.isInvalidated).toBe(false);
  queries.removeQueries({
    predicate: (query) =>
      ownsDeletionQuery(query, {
        accountId: source.owner.accountId,
        subjectId: subject,
        apiOrigin: origin,
      }),
  });
  for (const key of originalKeys)
    expect(queries.getQueryData(key)).toBeUndefined();
  for (const key of foreignKeys)
    expect(queries.getQueryData(key)).toEqual({ private: "foreign" });
  expect(tradeQueryOptions(source, null).enabled).toBe(false);
  expect(listingQueryOptions(source, "invalid").enabled).toBe(false);
  expect(inventoryQueryOptions(null).enabled).toBe(false);
  expect(packQuoteQueryOptions(source, opening, "1").queryKey).not.toEqual(
    packQuoteQueryOptions(source, opening, "2").queryKey,
  );
  expect(packQuoteQueryOptions(source, opening, "0").enabled).toBe(false);

  expect(tradesQueryOptions(source, { limit: 9 }).enabled).toBe(false);
  queries.clear();
});
