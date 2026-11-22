import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "../data/account";
import { finishLabel } from "./card-sources";
import { useEconomyCommand } from "../data/economy-commands";
import type { EconomyJournal } from "../data/economy-recovery";
import { usePackOutcome, usePackProducts } from "../data/economy-queries";
import {
  READ_ATTEMPTS,
  useInventoryGap,
  useWholeInventory,
} from "./connected-inventory";
import type {
  Opening,
  OpeningStage,
  OwnedPack,
  PackCard,
  PacksSource,
} from "./pack-sources";

const LOAD_FAILED = "Couldn’t load packs.";
const REJECTED = "This pack is no longer available.";
const RESTRICTED = "Pack opening is unavailable.";
const RETRY_PAUSE_MS = 500;

type Target = { assetId: string; product: string };
type Owner = { origin: string; accountId: string };

/** Whether an earlier opening of this pack never got its answer. Reading it sends nothing. */
function hasUnresolvedOpening(owner: Owner, assetId: string): boolean {
  try {
    const raw = window.localStorage.getItem(
      `spellwake.economy-command.v1:${owner.origin}:${owner.accountId}:pack_open:${assetId}`,
    );
    return (
      raw !== null &&
      (JSON.parse(raw) as { state?: string }).state === "unresolved"
    );
  } catch {
    return false;
  }
}

/** Card names for an opening's printings: the printings first, then their cards. */
function useOpeningCards(
  contents: readonly { printingId: string; quantity: string }[] | null,
) {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const printingIds = contents?.map((card) => card.printingId) ?? [];
  return useQuery({
    queryKey: [
      "opening-cards",
      owner?.accountId,
      owner?.sessionId,
      printingIds,
    ],
    enabled: !!owner && !!services && printingIds.length > 0,
    staleTime: Infinity,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }): Promise<PackCard[]> => {
      if (!owner || !services || !contents)
        throw new Error("Sign in to open packs.");
      const look = async (definitionIds: string[], ids: string[]) => {
        const result = await services.api.catalogue(
          await ownerBearer(services, owner),
          { definitionIds, printingIds: ids },
          signal,
        );
        if (result.status !== "success")
          throw new Error("Cards could not be named.");
        return result.data;
      };
      const printings = new Map<
        string,
        { definitionId: string; finish: string }
      >();
      for (let at = 0; at < printingIds.length; at += 100)
        for (const printing of (await look([], printingIds.slice(at, at + 100)))
          .printings)
          printings.set(printing.id, printing);
      const definitionIds = [
        ...new Set([...printings.values()].map((p) => p.definitionId)),
      ];
      const names = new Map<string, string>();
      for (let at = 0; at < definitionIds.length; at += 100)
        for (const card of (await look(definitionIds.slice(at, at + 100), []))
          .cards)
          names.set(card.id, card.name);
      // Duplicates arrive as separate cards, never as a count.
      return contents.flatMap((card) => {
        const printing = printings.get(card.printingId);
        const name = printing ? names.get(printing.definitionId) : undefined;
        return Array.from({ length: Number(card.quantity) }, (_, copy) => ({
          key: `${card.printingId}-${copy}`,
          name: name ?? "Unknown card",
          faceUrl: null,
          finish: finishLabel(printing?.finish ?? "nonfoil"),
        }));
      });
    },
  });
}

/**
 * The account's packs and one opening at a time. An opening is sent once,
 * under the existing journaled economy command. If its outcome is unknown the
 * stage stays open and only Check opening asks about that same attempt; a
 * reload restores that state without sending anything. The catalogue has no
 * pack artwork, so wrappers are drawn in code, and no card artwork, so the
 * opened cards show quiet frames with their names.
 */
export function useConnectedPacks(): PacksSource {
  const gap = useInventoryGap();
  const inventory = useWholeInventory();
  const products = usePackProducts();
  const { state, services } = useAccount();
  const session =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const owner = useMemo(
    () =>
      session && services
        ? { origin: services.api.origin, accountId: session.accountId }
        : null,
    [session?.accountId, services],
  );
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = products;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const [target, setTarget] = useState<Target | null>(null);
  const [stage, setStage] = useState<OpeningStage | null>(null);
  const [outcomeId, setOutcomeId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [isLoadNoticeDismissed, setIsLoadNoticeDismissed] = useState(false);
  const [wantsSubmit, setWantsSubmit] = useState(false);
  const outcomeTries = useRef(0);
  const command = useEconomyCommand(
    target ? { kind: "pack_open", targetId: target.assetId } : null,
  );
  const outcome = usePackOutcome(outcomeId, outcomeId !== null);
  const contents = outcome.data?.outcome.contents ?? null;
  const opened = useOpeningCards(contents);

  const packs = useMemo((): OwnedPack[] => {
    const named = new Map(
      (products.data?.pages ?? []).flatMap((page) =>
        page.items.map((product) => [product.id, product] as const),
      ),
    );
    return inventory.pages.flatMap((items) =>
      items.flatMap((item): OwnedPack[] => {
        if (item.asset.kind !== "pack") return [];
        const product = named.get(item.asset.recipeId);
        return [
          {
            id: item.assetId,
            setName: product?.setName ?? "Sealed pack",
            product: product?.name ?? "Sealed pack",
            setCode: null,
            owned: Number(item.owned),
            held: Number(item.held),
            available: Number(item.available),
          },
        ];
      }),
    );
  }, [inventory.pages, products.data]);

  const close = useCallback((message: string | null) => {
    setTarget(null);
    setStage(null);
    setOutcomeId(null);
    setWantsSubmit(false);
    if (message) setNotice(message);
  }, []);
  /** Where an answered attempt leads: its cards, a closed stage, or Check opening. */
  const settle = useCallback(
    (journal: EconomyJournal | null) => {
      if (journal?.state === "accepted") {
        outcomeTries.current = 0;
        setOutcomeId(journal.ack.resourceId);
        setStage("loading");
      } else if (journal?.state === "rejected") {
        close("account" in journal.errors ? RESTRICTED : REJECTED);
      } else setStage("unconfirmed");
    },
    [close],
  );

  // A reload restores an opening that never got its answer, and sends nothing.
  useEffect(() => {
    if (!owner || target) return;
    const waiting = packs.find((pack) => hasUnresolvedOpening(owner, pack.id));
    if (waiting) {
      setTarget({ assetId: waiting.id, product: waiting.product });
      setStage("unconfirmed");
    }
  }, [owner, packs, target]);

  // The first attempt is sent once its pack's journal is in hand.
  const { journal, run } = command;
  useEffect(() => {
    if (!wantsSubmit || !target) return;
    setWantsSubmit(false);
    if (journal.status === "unavailable") return close(RESTRICTED);
    const prior = journal.status === "saved" ? journal.value : null;
    // An earlier attempt for this pack is still unanswered: it is settled first.
    if (prior?.state === "unresolved") return setStage("unconfirmed");
    run.mutate(
      {
        type: "submit",
        expected: prior,
        draft: {
          kind: "pack_open",
          command: {
            requestId: crypto.randomUUID(),
            inventoryAssetId: target.assetId,
          },
        },
      },
      { onSuccess: settle, onError: () => setStage("unconfirmed") },
    );
  }, [wantsSubmit, target, journal, run, settle, close]);

  // The opening's committed receipt, read again up to three tries in all.
  const { isError: outcomeFailed, refetch: refetchOutcome } = outcome;
  useEffect(() => {
    if (!outcomeFailed) return;
    if (outcomeTries.current + 1 >= READ_ATTEMPTS) {
      setStage("unconfirmed");
      return;
    }
    const timer = window.setTimeout(() => {
      outcomeTries.current += 1;
      void refetchOutcome();
    }, RETRY_PAUSE_MS);
    return () => window.clearTimeout(timer);
  }, [outcomeFailed, refetchOutcome]);
  useEffect(() => {
    if (opened.data && stage === "loading") setStage("opened");
    if (opened.isError && stage === "loading") setStage("unconfirmed");
  }, [opened.data, opened.isError, stage]);

  const checkOpening = useCallback(() => {
    if (outcomeId) {
      // The opening was accepted: only its receipt is read again.
      outcomeTries.current = 0;
      setStage("loading");
      void refetchOutcome();
      void opened.refetch();
      return;
    }
    if (journal.status !== "saved" || journal.value.state !== "unresolved") {
      settle(journal.status === "saved" ? journal.value : null);
      if (journal.status !== "saved") close(RESTRICTED);
      return;
    }
    setStage("checking");
    run.mutate(
      { type: "recover", expected: journal.value },
      { onSuccess: settle, onError: () => setStage("unconfirmed") },
    );
  }, [outcomeId, journal, run, settle, close, refetchOutcome, opened]);

  const opening: Opening | null =
    target && stage
      ? {
          packId: target.assetId,
          product: target.product,
          stage,
          cards: stage === "opened" ? (opened.data ?? null) : null,
        }
      : null;

  if (gap) return { status: gap };
  if (inventory.isPending) return { status: "loading" };
  const isUnread = inventory.isExhausted && inventory.pages.length === 0;
  return {
    status: "ready",
    packs,
    isAppending: inventory.isAppending,
    isUnread,
    notice:
      notice ??
      (inventory.isExhausted && !isLoadNoticeDismissed ? LOAD_FAILED : null),
    dismissNotice: () => {
      if (notice) setNotice(null);
      else setIsLoadNoticeDismissed(true);
    },
    canOpen: session?.state === "active",
    opening,
    open: (packId) => {
      const pack = packs.find((candidate) => candidate.id === packId);
      // Open another starts from a resolved opening; anything unresolved blocks a new one.
      if (!pack || pack.available < 1 || (target && stage !== "opened")) return;
      setOutcomeId(null);
      setTarget({ assetId: pack.id, product: pack.product });
      setStage("opening");
      setWantsSubmit(true);
    },
    checkOpening,
    closeOpening: () => {
      if (stage === "opened") close(null);
    },
  };
}
