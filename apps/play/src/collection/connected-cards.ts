import { scryfallImage } from "../data/scryfall-images";
import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";
import type { CatalogueLabels } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "../data/account";
import {
  finishLabel,
  orderPrintings,
  RARITIES,
  type CardsSource,
  type OwnedCard,
  type OwnedPrinting,
  type Rarity,
} from "./card-sources";
import {
  READ_ATTEMPTS,
  useInventoryGap,
  useWholeInventory,
} from "./connected-inventory";

const LOAD_FAILED = "Couldn’t load your cards.";
const MORE_FAILED = "Couldn’t load more cards.";
const LOOKUP_SIZE = 100;

const rarityOf = (rarity: string): Rarity | null =>
  (RARITIES as readonly string[]).includes(rarity) ? (rarity as Rarity) : null;

/** Names and printing labels for one page of owned printings: the printings first, then their cards. */
export function usePageLabels(pages: readonly (readonly string[])[]) {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  return useQueries({
    queries: pages.map((printingIds) => ({
      queryKey: [
        "collection-labels",
        owner?.accountId,
        owner?.sessionId,
        printingIds,
      ],
      enabled: !!owner && !!services && printingIds.length > 0,
      staleTime: Infinity,
      retry: READ_ATTEMPTS - 1,
      queryFn: async ({
        signal,
      }: {
        signal: AbortSignal;
      }): Promise<CatalogueLabels> => {
        if (!owner || !services) throw new Error("Sign in to load cards.");
        const look = async (definitionIds: string[], ids: string[]) => {
          const result = await services.api.catalogue(
            await ownerBearer(services, owner),
            { definitionIds, printingIds: ids },
            signal,
          );
          if (result.status !== "success")
            throw new Error("Card names could not be loaded.");
          return result.data;
        };
        const printings = (await look([], [...printingIds])).printings;
        const definitionIds = [
          ...new Set(printings.map((printing) => printing.definitionId)),
        ];
        const cards: CatalogueLabels["cards"] = [];
        for (let at = 0; at < definitionIds.length; at += LOOKUP_SIZE)
          cards.push(
            ...(await look(definitionIds.slice(at, at + LOOKUP_SIZE), []))
              .cards,
          );
        return { cards, printings };
      },
    })),
    // Combined results keep their identity until a page's data changes.
    combine: (results) => ({
      pages: results.map((result) => result.data),
      isError: results.some((result) => result.isError),
      isPending: results.some(
        (result, index) => result.isPending && (pages[index]?.length ?? 0) > 0,
      ),
    }),
  });
}

/**
 * The account's owned cards. The catalogue names each printing's card, set,
 * collector number, finish and rarity, and its Scryfall card ID, which gives
 * the official card image. It has no type, colour, mana value or legality,
 * so the screen offers only the set and rarity filters and the name, set and
 * owned sorts.
 */
export function useConnectedCards(): CardsSource {
  const gap = useInventoryGap();
  const inventory = useWholeInventory();
  const printingPages = useMemo(
    () =>
      inventory.pages.map((items) =>
        items.flatMap((item) =>
          item.asset.kind === "card" ? [item.asset.printingId] : [],
        ),
      ),
    [inventory.pages],
  );
  const labels = usePageLabels(printingPages);
  const labelsFailed = labels.isError;
  const labelsPending = labels.isPending;
  const labelData = labels.pages;
  const cards = useMemo(() => {
    const owned = new Map<string, number>();
    for (const items of inventory.pages)
      for (const item of items)
        if (item.asset.kind === "card")
          // Only copies usable now: those held by trades or listings are left out.
          owned.set(item.asset.printingId, Number(item.available));
    const names = new Map<string, string>();
    const printingsByCard = new Map<string, OwnedPrinting[]>();
    for (const page of labelData) {
      for (const card of page?.cards ?? []) names.set(card.id, card.name);
      for (const printing of page?.printings ?? []) {
        const copies = owned.get(printing.id);
        if (!copies) continue;
        const list = printingsByCard.get(printing.definitionId) ?? [];
        list.push({
          id: printing.id,
          setCode: printing.set.code?.toUpperCase() ?? null,
          setName: printing.set.name,
          number: printing.collectorNumber,
          finish: finishLabel(printing.finish),
          rarity: rarityOf(printing.rarity),
          copies,
          faceUrl: scryfallImage(printing.externalPrintingRef, "normal"),
          backUrl: null,
        });
        printingsByCard.set(printing.definitionId, list);
      }
    }
    // A card whose name is not yet known is left out until it is.
    return [...printingsByCard].flatMap(([id, printings]): OwnedCard[] => {
      const name = names.get(id);
      if (!name) return [];
      const ordered = orderPrintings(printings);
      return [
        {
          id,
          name,
          copies: ordered.reduce((sum, printing) => sum + printing.copies, 0),
          printings: ordered,
          type: null,
          rules: null,
          manaValue: null,
          colors: null,
          identity: null,
          legality: null,
        },
      ];
    });
  }, [inventory.pages, labelData]);

  if (gap) return { status: gap };
  if (inventory.isPending || (cards.length === 0 && labelsPending))
    return { status: "loading" };
  const isUnread = inventory.isExhausted && inventory.pages.length === 0;
  return {
    status: "ready",
    cards,
    facets: {
      formats: null,
      color: false,
      type: false,
      rarity: true,
      manaValue: false,
    },
    isAppending: inventory.isAppending || labelsPending,
    failure:
      inventory.isExhausted || labelsFailed
        ? isUnread || cards.length === 0
          ? LOAD_FAILED
          : MORE_FAILED
        : null,
    isUnread: isUnread || (labelsFailed && cards.length === 0),
  };
}
