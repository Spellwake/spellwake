import type { QueryClient } from "@tanstack/react-query";
/** Exact API/account prefix shared by economy reads, mutation refresh and terminal erasure. */
export const economyQueryFamilies = [
  "economy-inventory",
  "trade-history",
  "trade-detail",
  "market-listings",
  "market-listing",
  "pack-outcome",
  "pack-products",
  "pack-quote",
] as const;
export type EconomyQueryFamily = (typeof economyQueryFamilies)[number];

/** Refresh hold-aware inventory, exchange receipts and dependent deck availability after confirmation. */
export async function refreshEconomyQueries(
  queries: QueryClient,
  accountId: string,
  origin: string,
): Promise<void> {
  await Promise.all([
    ...economyQueryFamilies.map((family) =>
      queries.invalidateQueries({ queryKey: [family, accountId, origin] }),
    ),
    ...["account-summary", "deck-library", "owned-deck"].map((family) =>
      queries.invalidateQueries({ queryKey: [family, accountId] }),
    ),
  ]);
}
