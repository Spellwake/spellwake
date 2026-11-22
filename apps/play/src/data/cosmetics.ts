import { useInfiniteQuery } from "@tanstack/react-query";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "./account";

/** Current grants are read afresh; browser checkout returns never establish ownership. */
export function useCosmeticCatalogue() {
  const { state, services } = useAccount();
  const owner = state.status === "ready" ? state.session : null;
  return useInfiniteQuery({
    queryKey: ["cosmetics", owner?.accountId, owner?.sessionId],
    enabled: !!owner?.accountId && !!services,
    initialPageParam: null as string | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services) throw new Error("Sign in to view cosmetics.");
      const result = await services.api.cosmetics(
        await ownerBearer(services, owner),
        pageParam,
        signal,
      );
      if (result.status !== "success")
        throw new Error("Your cosmetics could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    refetchInterval: 15000,
  });
}
/** All pages remain owner-scoped, including incomplete and reversed purchases. */
export function usePurchaseHistory() {
  const { state, services } = useAccount();
  const owner = state.status === "ready" ? state.session : null;
  return useInfiniteQuery({
    queryKey: ["purchases", owner?.accountId, owner?.sessionId],
    enabled: !!owner?.accountId && !!services,
    initialPageParam: null as string | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services) throw new Error("Sign in to view purchases.");
      const result = await services.api.purchases(
        await ownerBearer(services, owner),
        pageParam,
        signal,
      );
      if (result.status !== "success")
        throw new Error("Your purchases could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    refetchInterval: 5000,
  });
}
