import {
  infiniteQueryOptions,
  queryOptions,
  useInfiniteQuery,
  useQueries,
  useQuery,
} from "@tanstack/react-query";
import {
  identifierSchema,
  packQuantitySchema,
  tradePageQuerySchema,
  marketPageQuerySchema,
  type TradePageQuery,
  type MarketPageQuery,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";
import type { ApiResponse } from "./application-api";
import { readForOwner, type OwnerReadScope } from "./owner-read";
import type { EconomyQueryFamily } from "./economy-cache";

export type EconomyReadScope = OwnerReadScope & {
  services: OwnerReadScope["services"] & Pick<ApplicationServices, "api">;
};
function key(scope: EconomyReadScope | null, family: EconomyQueryFamily) {
  return [
    family,
    scope?.owner.accountId,
    scope?.services.api.origin,
    scope?.owner.subjectId,
    scope?.owner.sessionId,
  ] as const;
}
async function projection<T>(
  scope: EconomyReadScope | null,
  signal: AbortSignal,
  read: (scope: EconomyReadScope, bearer: string) => Promise<ApiResponse<T>>,
): Promise<T> {
  if (!scope) throw new Error("Sign in to load this view.");
  const result = await readForOwner(scope, signal, (bearer) =>
    read(scope, bearer),
  );
  if (result.status !== "success")
    throw new Error("This view could not be loaded.");
  return result.data;
}
function pageOptions<T extends { nextCursor: string | null }>(
  scope: EconomyReadScope | null,
  family: EconomyQueryFamily,
  filter: unknown,
  enabled: boolean,
  read: (
    scope: EconomyReadScope,
    bearer: string,
    after: string | null,
    signal: AbortSignal,
  ) => Promise<ApiResponse<T>>,
) {
  return infiniteQueryOptions({
    queryKey: [...key(scope, family), filter],
    enabled: enabled && scope !== null,
    initialPageParam: null as string | null,
    queryFn: ({ signal, pageParam }) =>
      projection(scope, signal, (source, bearer) =>
        read(source, bearer, pageParam, signal),
      ),
    getNextPageParam: (page: T) => page.nextCursor ?? undefined,
    retry: false,
    refetchOnWindowFocus: true,
  });
}
/** Available/held/owned quantities remain independent exact server values across pages. */
export function inventoryQueryOptions(
  scope: EconomyReadScope | null,
  enabled = true,
) {
  return pageOptions(
    scope,
    "economy-inventory",
    null,
    enabled,
    (source, bearer, after, signal) =>
      source.services.api.inventory(bearer, after, signal),
  );
}
/** Filter and page size participate in cache identity; cursors belong to each infinite query. */
export function tradesQueryOptions(
  scope: EconomyReadScope | null,
  query: Omit<TradePageQuery, "after"> = {},
  enabled = true,
) {
  const parsed = tradePageQuerySchema.omit({ after: true }).safeParse(query);
  return pageOptions(
    scope,
    "trade-history",
    parsed.success ? parsed.data : query,
    enabled && parsed.success,
    (source, bearer, after, signal) => {
      if (!parsed.success) throw new Error("Trade filters are unavailable.");
      return source.services.api.trades(
        bearer,
        source.owner.accountId,
        { ...parsed.data, after },
        signal,
      );
    },
  );
}
/** Public lots and participant history never share cached pages. */
export function marketQueryOptions(
  scope: EconomyReadScope | null,
  query: Omit<MarketPageQuery, "after"> = {},
  enabled = true,
) {
  const parsed = marketPageQuerySchema.omit({ after: true }).safeParse(query);
  return pageOptions(
    scope,
    "market-listings",
    parsed.success ? parsed.data : query,
    enabled && parsed.success,
    (source, bearer, after, signal) => {
      if (!parsed.success) throw new Error("Market filters are unavailable.");
      return source.services.api.marketListings(
        bearer,
        source.owner.accountId,
        { ...parsed.data, after },
        signal,
      );
    },
  );
}
function detailOptions<T>(
  scope: EconomyReadScope | null,
  family: EconomyQueryFamily,
  id: string | null,
  enabled: boolean,
  read: (
    scope: EconomyReadScope,
    bearer: string,
    id: string,
    signal: AbortSignal,
  ) => Promise<ApiResponse<T>>,
) {
  return queryOptions({
    queryKey: [...key(scope, family), id],
    enabled:
      enabled && scope !== null && identifierSchema.safeParse(id).success,
    queryFn: ({ signal }) =>
      projection(scope, signal, (source, bearer) => {
        if (!identifierSchema.safeParse(id).success || id === null)
          throw new Error("Choose an item first.");
        return read(source, bearer, id, signal);
      }),
    retry: false,
    refetchOnWindowFocus: true,
  });
}
/** A detail refresh preserves the exact requested trade; it never selects another offer. */
export function tradeQueryOptions(
  scope: EconomyReadScope | null,
  id: string | null,
  enabled = true,
) {
  return detailOptions(
    scope,
    "trade-detail",
    id,
    enabled,
    (source, bearer, target, signal) =>
      source.services.api.trade(bearer, source.owner.accountId, target, signal),
  );
}
/** Visibility is revalidated on each request, including terminal participant-only history. */
export function listingQueryOptions(
  scope: EconomyReadScope | null,
  id: string | null,
  enabled = true,
) {
  return detailOptions(
    scope,
    "market-listing",
    id,
    enabled,
    (source, bearer, target, signal) =>
      source.services.api.marketListing(
        bearer,
        source.owner.accountId,
        target,
        signal,
      ),
  );
}
/** Contents come only from the caller-owned committed receipt; randomness stays server-side. */
export function packOutcomeQueryOptions(
  scope: EconomyReadScope | null,
  id: string | null,
  enabled = true,
) {
  return detailOptions(
    scope,
    "pack-outcome",
    id,
    enabled,
    (source, bearer, target, signal) =>
      source.services.api.packOutcome(
        bearer,
        source.owner.accountId,
        target,
        signal,
      ),
  );
}
function useEconomyScope(): EconomyReadScope | null {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  return owner && services
    ? { owner, services, isCurrent: () => isCurrentSession(owner) }
    : null;
}
export function useEconomicInventory(enabled = true) {
  return useInfiniteQuery(inventoryQueryOptions(useEconomyScope(), enabled));
}
export function useTrades(
  query: Omit<TradePageQuery, "after"> = {},
  enabled = true,
) {
  return useInfiniteQuery(
    tradesQueryOptions(useEconomyScope(), query, enabled),
  );
}
export function useMarketListings(
  query: Omit<MarketPageQuery, "after"> = {},
  enabled = true,
) {
  return useInfiniteQuery(
    marketQueryOptions(useEconomyScope(), query, enabled),
  );
}
export function useTrade(id: string | null, enabled = true) {
  return useQuery(tradeQueryOptions(useEconomyScope(), id, enabled));
}
export function useMarketListing(id: string | null, enabled = true) {
  return useQuery(listingQueryOptions(useEconomyScope(), id, enabled));
}
export function usePackOutcome(id: string | null, enabled = true) {
  return useQuery(packOutcomeQueryOptions(useEconomyScope(), id, enabled));
}

/** Product labels remain distinct from quantity-specific current quotes. */
export function packProductsQueryOptions(
  scope: EconomyReadScope | null,
  enabled = true,
) {
  return pageOptions(
    scope,
    "pack-products",
    null,
    enabled,
    (source, bearer, after, signal) =>
      source.services.api.packProducts(
        bearer,
        source.owner.accountId,
        after,
        signal,
      ),
  );
}
/** A changed quantity gets its own cache entry; purchase still uses explicit observed terms. */
export function packQuoteQueryOptions(
  scope: EconomyReadScope | null,
  recipeId: string | null,
  quantity = "1",
  enabled = true,
) {
  const options = detailOptions(
    scope,
    "pack-quote",
    recipeId,
    enabled && packQuantitySchema.safeParse(quantity).success,
    (source, bearer, target, signal) =>
      source.services.api.packQuote(
        bearer,
        source.owner.accountId,
        target,
        quantity,
        signal,
      ),
  );
  return queryOptions({
    ...options,
    queryKey: [...options.queryKey, quantity],
  });
}
export function usePackProducts(enabled = true) {
  return useInfiniteQuery(packProductsQueryOptions(useEconomyScope(), enabled));
}
export function usePackQuote(
  recipeId: string | null,
  quantity = "1",
  enabled = true,
) {
  return useQuery(
    packQuoteQueryOptions(useEconomyScope(), recipeId, quantity, enabled),
  );
}
/** Quantity-one quotes for many products at once, as a supply gallery shows them. */
export function usePackQuotes(recipeIds: readonly string[], enabled = true) {
  const scope = useEconomyScope();
  return useQueries({
    queries: recipeIds.map((recipeId) =>
      packQuoteQueryOptions(scope, recipeId, "1", enabled),
    ),
  });
}
