import { useEffect, useMemo, useState } from "react";
import type { InventoryPage } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { useEconomicInventory } from "../data/economy-queries";

/** Total tries at each inventory read, the first included. */
export const READ_ATTEMPTS = 3;
const RETRY_PAUSE_MS = 500;

export type InventoryItem = InventoryPage["items"][number];
export type WholeInventory = {
  /** Every page read so far, in order. */
  pages: readonly (readonly InventoryItem[])[];
  /** Nothing read yet, and still trying. */
  isPending: boolean;
  /** Pages remain, and are being read. */
  isAppending: boolean;
  /** A read failed on every try. Pages already read are kept. */
  isExhausted: boolean;
};

/** Why the account has nothing to read, or null when it can be read. */
export function useInventoryGap():
  "loading" | "signed-out" | "unregistered" | "unavailable" | null {
  const { state } = useAccount();
  if (state.status === "loading") return "loading";
  if (state.status === "signed_out") return "signed-out";
  if (state.status !== "ready") return "unavailable";
  return state.session.accountId ? null : "unregistered";
}

/**
 * The account's whole inventory, page after page, so search and filters cover
 * all of it. A failed page is tried again, up to three tries in all, and then
 * left: what was read stays, and nothing claims the rest is empty.
 */
export function useWholeInventory(): WholeInventory {
  const inventory = useEconomicInventory();
  const [failures, setFailures] = useState(0);
  const { data, isError, isFetching, hasNextPage, fetchNextPage, refetch } =
    inventory;
  const hasData = data !== undefined;
  const isExhausted = isError && !isFetching && failures >= READ_ATTEMPTS - 1;
  useEffect(() => {
    if (isFetching) return;
    if (isError) {
      if (failures >= READ_ATTEMPTS - 1) return;
      const timer = window.setTimeout(
        () => {
          setFailures((count) => count + 1);
          void (hasData ? fetchNextPage() : refetch());
        },
        RETRY_PAUSE_MS * (failures + 1),
      );
      return () => window.clearTimeout(timer);
    }
    if (failures > 0) setFailures(0);
    if (hasNextPage) void fetchNextPage();
    return undefined;
  }, [
    isFetching,
    isError,
    failures,
    hasData,
    hasNextPage,
    fetchNextPage,
    refetch,
  ]);
  const pages = useMemo(
    () => (data?.pages ?? []).map((page) => page.items),
    [data],
  );
  return {
    pages,
    isPending: !hasData && !isExhausted,
    isAppending: hasData && !!hasNextPage && !isExhausted,
    isExhausted,
  };
}
