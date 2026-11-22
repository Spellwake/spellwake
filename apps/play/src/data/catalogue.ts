import { useInfiniteQuery, useMutation, useQuery } from "@tanstack/react-query";
import type { CatalogueLabels } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "./account";
import { parseDeckIntake } from "./deck-text";
import { resolveDeckLines } from "./deck-resolution";

/** Text parsing and canonical lookup do not write deck state. */
export function useDeckImport() {
  const { state, services, isCurrentSession } = useAccount();
  const owner = state.status === "ready" ? state.session : null;
  return useMutation({
    mutationFn: async (text: string) => {
      const intake = parseDeckIntake(text);
      if (intake.status === "fail") return intake;
      if (!owner || !services) throw new Error("Sign in to resolve cards.");
      const lines = await resolveDeckLines(
        services.api,
        await ownerBearer(services, owner),
        intake.lines,
      );
      if (!isCurrentSession(owner))
        throw new Error("Your session changed. Reopen the deck.");
      return { status: "success", lines } as const;
    },
  });
}

/** Search labels without interpreting a typed name as a canonical identity. */
export function useCardSearch(query: string, exact = false) {
  const { state, services } = useAccount();
  const owner = state.status === "ready" ? state.session : null;
  return useInfiniteQuery({
    queryKey: ["card-search", owner?.accountId, owner?.sessionId, query, exact],
    enabled: !!owner?.accountId && !!services && query.trim().length > 0,
    initialPageParam: null as string | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services) throw new Error("Sign in to find cards.");
      const result = await services.api.cards(
        await ownerBearer(services, owner),
        query,
        exact,
        pageParam,
        signal,
      );
      if (result.status !== "success")
        throw new Error("Cards could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
  });
}

/** Exact-name lookup keeps every distinct finish and definition available for selection. */
export function usePrintingSearch(name: string | null) {
  const { state, services } = useAccount();
  const owner = state.status === "ready" ? state.session : null;
  return useInfiniteQuery({
    queryKey: ["printing-search", owner?.accountId, owner?.sessionId, name],
    enabled: !!owner?.accountId && !!services && name !== null,
    initialPageParam: null as string | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services || name === null)
        throw new Error("Choose a card first.");
      const result = await services.api.namedPrintings(
        await ownerBearer(services, owner),
        name,
        {},
        pageParam,
        signal,
      );
      if (result.status !== "success")
        throw new Error("Printings could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
  });
}

/** Resolve bounded batches separately from immutable deck terms; missing labels stay missing. */
export function useDeckLabels(
  entries: readonly { definitionId: string; printingId: string | null }[],
) {
  const { state, services } = useAccount();
  const owner = state.status === "ready" ? state.session : null;
  const definitionIds = [
    ...new Set(entries.map((entry) => entry.definitionId)),
  ].sort();
  const printingIds = [
    ...new Set(
      entries.flatMap((entry) => (entry.printingId ? [entry.printingId] : [])),
    ),
  ].sort();
  return useQuery({
    queryKey: [
      "deck-labels",
      owner?.accountId,
      owner?.sessionId,
      definitionIds,
      printingIds,
    ],
    enabled: !!owner?.accountId && !!services,
    queryFn: async ({ signal }): Promise<CatalogueLabels> => {
      if (!owner || !services) throw new Error("Sign in to load card names.");
      const labels: CatalogueLabels = { cards: [], printings: [] };
      for (
        let offset = 0;
        offset < Math.max(definitionIds.length, printingIds.length);
        offset += 100
      ) {
        const result = await services.api.catalogue(
          await ownerBearer(services, owner),
          {
            definitionIds: definitionIds.slice(offset, offset + 100),
            printingIds: printingIds.slice(offset, offset + 100),
          },
          signal,
        );
        if (result.status !== "success")
          throw new Error("Card names could not be loaded.");
        labels.cards.push(...result.data.cards);
        labels.printings.push(...result.data.printings);
      }
      return labels;
    },
  });
}
