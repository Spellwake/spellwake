import { useCallback, useEffect, useMemo, useState } from "react";
import {
  useInfiniteQuery,
  useQueries,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import type {
  PredictionMarketSummary,
  PredictionPosition,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer, useAccountSummary } from "../data/account";
import { usePredictionCommand } from "../data/prediction-commands";
import type { PredictionJournal } from "../data/prediction-commands";
import { scryfallImage } from "../data/scryfall-images";
import { finishLabel } from "../collection/card-sources";
import { usePageLabels } from "../collection/connected-cards";
import {
  READ_ATTEMPTS,
  useInventoryGap,
} from "../collection/connected-inventory";
import {
  canonicalTarget,
  publicationTimes,
  utcDay,
  type Completed,
  type MyBet,
  type Prediction,
  type PredictionDraft,
  type PredictionsSource,
  type PrintingLabel,
  type PriceCheck,
  type ReadyPredictions,
} from "./prediction-sources";

const LOAD_FAILED = "Could not load predictions";
const UPDATE_FAILED = "Could not update predictions";
const CREATE_FAILED = "Couldn’t create prediction.";
const LOOKUP_SIZE = 100;
const PREPARE_POLLS = 10;
const PREPARE_PAUSE_MS = 2000;

/** A wire amount as a number, or null when it is too large to show exactly. */
const exact = (amount: bigint): number | null =>
  amount <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(amount) : null;

/**
 * One owner's matched positions on one market as the single bet the design
 * shows. Positions on both sides, or amounts too large to show exactly, are
 * not one bet, and no single result is claimed for them.
 */
export function betFromPositions(
  positions: readonly Pick<
    PredictionPosition,
    "side" | "quantity" | "actualCost"
  >[],
): MyBet {
  if (positions.length === 0) return { kind: "none" };
  const side = positions[0]!.side;
  if (positions.some((position) => position.side !== side))
    return { kind: "unverified" };
  let cost = 0n;
  let quantity = 0n;
  for (const position of positions) {
    cost += BigInt(position.actualCost);
    quantity += BigInt(position.quantity);
  }
  const shownCost = exact(cost);
  const win = exact(quantity * 100n);
  return shownCost === null || win === null
    ? { kind: "unverified" }
    : { kind: "bet", side, cost: shownCost, win };
}

function useOwner() {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  return { owner, services };
}

/** Every page of an owner read, fetched one after another. */
function useAllPages<Item>(
  family: string,
  read: (
    bearer: string,
    accountId: string,
    after: string | null,
    signal: AbortSignal,
  ) => Promise<
    | { status: "success"; data: { items: Item[]; nextCursor: string | null } }
    | { status: "fail" | "error" }
  >,
  automatic: boolean,
) {
  const { owner, services } = useOwner();
  const query = useInfiniteQuery({
    queryKey: ["predictions", owner?.accountId, services?.api.origin, family],
    enabled: !!owner && !!services,
    initialPageParam: null as string | null,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ pageParam, signal }) => {
      if (!owner || !services) throw new Error("Sign in to see predictions.");
      const result = await read(
        await ownerBearer(services, owner),
        owner.accountId,
        pageParam,
        signal,
      );
      if (result.status !== "success") throw new Error(LOAD_FAILED);
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
  });
  const { hasNextPage, isFetchingNextPage, isError, fetchNextPage } = query;
  useEffect(() => {
    if (automatic && hasNextPage && !isFetchingNextPage && !isError)
      void fetchNextPage();
  }, [automatic, hasNextPage, isFetchingNextPage, isError, fetchNextPage]);
  return query;
}

/** Card search for a new prediction: matching cards, then each card's printings. */
function useCardSearch(query: string | null) {
  const { owner, services } = useOwner();
  const needle = query?.trim() ?? "";
  const cards = useInfiniteQuery({
    queryKey: [
      "prediction-cards",
      owner?.accountId,
      services?.api.origin,
      needle,
    ],
    enabled: !!owner && !!services && query !== null && needle.length > 0,
    initialPageParam: null as string | null,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ pageParam, signal }) => {
      if (!owner || !services) throw new Error("Sign in to search cards.");
      const result = await services.api.cards(
        await ownerBearer(services, owner),
        needle,
        false,
        pageParam,
        signal,
      );
      if (result.status !== "success") throw new Error("Couldn’t load cards.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
  });
  const found = useMemo(
    () => (cards.data?.pages ?? []).flatMap((page) => page.items),
    [cards.data],
  );
  const printings = useQueries({
    queries: found.map((card) => ({
      queryKey: ["prediction-printings", owner?.accountId, card.id],
      enabled: !!owner && !!services,
      staleTime: Infinity,
      retry: READ_ATTEMPTS - 1,
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        if (!owner || !services) throw new Error("Sign in to search cards.");
        const result = await services.api.printings(
          await ownerBearer(services, owner),
          card.id,
          null,
          signal,
        );
        if (result.status !== "success")
          throw new Error("Couldn’t load cards.");
        return result.data.items.map((printing): PrintingLabel => ({
          printingId: printing.id,
          name: card.name,
          setCode: printing.set.code?.toUpperCase() ?? null,
          finish: finishLabel(printing.finish),
          faceUrl: scryfallImage(printing.externalPrintingRef, "art_crop"),
        }));
      },
    })),
    combine: (results) => ({
      items: results.flatMap((result) => result.data ?? []),
      isError: results.some((result) => result.isError),
      isPending: results.some((result) => result.isPending),
    }),
  });
  return { cards, printings, needle };
}

/**
 * The exact printing's price source for a new prediction. A printing with no
 * usable price yet is prepared from the retained daily source, then read
 * again; reading alone never creates one.
 */
function usePriceCheck(printingId: string | null) {
  const { owner, services } = useOwner();
  return useQuery({
    queryKey: ["prediction-source", owner?.accountId, printingId],
    enabled: !!owner && !!services && printingId !== null,
    retry: READ_ATTEMPTS - 1,
    staleTime: 60_000,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !printingId)
        throw new Error("Sign in to check prices.");
      const read = async () =>
        services.api.predictionPublicationSource(
          await ownerBearer(services, owner),
          owner.accountId,
          printingId,
          signal,
        );
      let result = await read();
      if (result.status === "success")
        return { state: "ready" as const, probeId: result.data.source.probeId };
      if (result.status === "fail" && "printing" in result.data)
        return { state: "unavailable" as const, probeId: null };
      if (result.status !== "fail" || !("price" in result.data))
        throw new Error("Couldn’t check the price.");
      for (let poll = 0; poll < PREPARE_POLLS; poll += 1) {
        const prepared = await services.api.preparePredictionSource(
          await ownerBearer(services, owner),
          owner.accountId,
          printingId,
          signal,
        );
        if (prepared.status !== "success")
          return { state: "unavailable" as const, probeId: null };
        if (prepared.data.state === "unavailable")
          return { state: "unavailable" as const, probeId: null };
        if (prepared.data.state === "completed") break;
        await new Promise((resolve) =>
          window.setTimeout(resolve, PREPARE_PAUSE_MS),
        );
      }
      result = await read();
      if (result.status === "success")
        return { state: "ready" as const, probeId: result.data.source.probeId };
      if (result.status === "fail")
        return { state: "unavailable" as const, probeId: null };
      throw new Error("Couldn’t check the price.");
    },
  });
}

/**
 * The account's Predictions. Explore reads every published prediction page
 * by page; Your bets joins the account's matched positions to their
 * predictions, with returns from the fixed 100 coins per winning contract.
 * Create prediction checks the exact printing's price source and publishes
 * one journaled `publish_market`; pressing Create again after a failure looks
 * up or resends that same request first.
 *
 * Not yet possible against this application, so shown as unavailable: a
 * bet, because its order can fill in part and leave a resting remainder
 * where the design takes the whole amount or nothing, once per prediction;
 * the questions this account created, which no read lists; and the dated
 * price a result rests on.
 */
export function useConnectedPredictions(): PredictionsSource {
  const gap = useInventoryGap();
  const { owner, services } = useOwner();
  const summary = useAccountSummary();
  const markets = useAllPages(
    "markets",
    (bearer, accountId, after, signal) =>
      services!.api.predictionMarkets(bearer, accountId, after, signal),
    false,
  );
  const positions = useAllPages(
    "positions",
    (bearer, accountId, after, signal) =>
      services!.api.predictionPositions(bearer, accountId, null, after, signal),
    true,
  );
  const marketList = useMemo(
    () => (markets.data?.pages ?? []).flatMap((page) => page.items),
    [markets.data],
  );
  const positionList = useMemo(
    () => (positions.data?.pages ?? []).flatMap((page) => page.items),
    [positions.data],
  );
  const positionsComplete =
    !!positions.data && !positions.hasNextPage && !positions.isError;

  // Predictions with a position that Explore has not reached yet.
  const known = new Set(marketList.map((market) => market.id));
  const missing = [
    ...new Set(positionList.map((position) => position.marketId)),
  ].filter((id) => !known.has(id));
  const details = useQueries({
    queries: missing.map((marketId) => ({
      queryKey: [
        "predictions",
        owner?.accountId,
        services?.api.origin,
        "market",
        marketId,
      ],
      enabled: !!owner && !!services,
      retry: READ_ATTEMPTS - 1,
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        if (!owner || !services) throw new Error(LOAD_FAILED);
        const result = await services.api.predictionMarket(
          await ownerBearer(services, owner),
          owner.accountId,
          marketId,
          signal,
        );
        if (result.status !== "success") throw new Error(LOAD_FAILED);
        return result.data.market.terms;
      },
    })),
    combine: (results) => ({
      terms: results.flatMap((result) => (result.data ? [result.data] : [])),
      isError: results.some((result) => result.isError),
    }),
  });

  const allTerms = useMemo(
    () => [...marketList, ...details.terms],
    [marketList, details.terms],
  );
  const printingPages = useMemo(() => {
    const ids = [...new Set(allTerms.map((terms) => terms.printingId))];
    const pages: string[][] = [];
    for (let at = 0; at < ids.length; at += LOOKUP_SIZE)
      pages.push(ids.slice(at, at + LOOKUP_SIZE));
    return pages;
  }, [allTerms]);
  const labels = usePageLabels(printingPages);
  const toPrediction = useCallback(
    (terms: PredictionMarketSummary): Prediction | null => {
      let printing = null;
      let name = null;
      for (const page of labels.pages) {
        printing ??=
          page?.printings.find((entry) => entry.id === terms.printingId) ??
          null;
      }
      if (!printing) return null;
      for (const page of labels.pages)
        name ??=
          page?.cards.find((card) => card.id === printing.definitionId)?.name ??
          null;
      if (!name) return null;
      const own = positionList.filter(
        (position) => position.marketId === terms.id,
      );
      return {
        id: terms.id,
        card: {
          printingId: terms.printingId,
          name,
          setCode: printing.set.code?.toUpperCase() ?? null,
          finish: finishLabel(printing.finish),
          faceUrl: scryfallImage(printing.externalPrintingRef, "art_crop"),
        },
        targetUsd: terms.targetUsd,
        condition: terms.condition,
        deadline: utcDay(terms.deadline),
        closesAt: terms.tradingClosesAt,
        state: terms.state,
        accepting: terms.acceptingOrders,
        outcome: terms.outcome,
        evidence: null,
        voidReason: null,
        mine:
          own.length > 0 || positionsComplete
            ? betFromPositions(own)
            : { kind: "unknown" },
        authored: false,
      };
    },
    [labels.pages, positionList, positionsComplete],
  );
  const explore = useMemo(
    () =>
      marketList.flatMap((terms) => {
        const prediction = toPrediction(terms);
        return prediction ? [prediction] : [];
      }),
    [marketList, toPrediction],
  );
  const bets = useMemo(
    () =>
      allTerms.flatMap((terms) => {
        const prediction = toPrediction(terms);
        return prediction &&
          prediction.mine.kind !== "none" &&
          prediction.mine.kind !== "unknown"
          ? [prediction]
          : [];
      }),
    [allTerms, toPrediction],
  );

  // Card search and the chosen printing's price source.
  const [cardQuery, setCardQuery] = useState<string | null>(null);
  const search = useCardSearch(cardQuery);
  const [pricePrinting, setPricePrinting] = useState<string | null>(null);
  const priceRead = usePriceCheck(pricePrinting);
  const searchCards = useCallback(
    (query: string | null) => setCardQuery(query),
    [],
  );
  const checkPrice = useCallback(
    (printingId: string | null) => setPricePrinting(printingId),
    [],
  );

  // One publication at a time, journaled under its printing.
  const [publishTarget, setPublishTarget] = useState<{
    draft: PredictionDraft;
    started: boolean;
  } | null>(null);
  const [publishPending, setPublishPending] = useState(false);
  const [commandFailure, setCommandFailure] = useState<string | null>(null);
  const [completed, setCompleted] = useState<Completed | null>(null);
  const [isFailureDismissed, setIsFailureDismissed] = useState(false);
  const { journal, run } = usePredictionCommand(
    publishTarget
      ? {
          kind: "publish_market",
          targetId: publishTarget.draft.card.printingId,
        }
      : null,
  );
  const settle = useCallback((result: PredictionJournal | null) => {
    setPublishPending(false);
    setPublishTarget(null);
    if (result?.state === "accepted") {
      setCompleted({ kind: "created", predictionId: result.ack.resourceId });
      return;
    }
    setCommandFailure(CREATE_FAILED);
  }, []);
  const probeId = priceRead.data?.probeId ?? null;
  useEffect(() => {
    if (!publishTarget || publishTarget.started) return;
    const { draft } = publishTarget;
    // Each press starts once; the journal answers it.
    setPublishTarget({ draft, started: true });
    if (journal.status === "unavailable") return settle(null);
    const prior = journal.status === "saved" ? journal.value : null;
    // An earlier attempt whose answer never came is settled first, unchanged.
    if (prior?.state === "unresolved") {
      run.mutate(
        { type: "recover", expected: prior },
        { onSuccess: settle, onError: () => settle(null) },
      );
      return;
    }
    if (!probeId || pricePrinting !== draft.card.printingId)
      return settle(null);
    run.mutate(
      {
        type: "submit",
        expected: prior,
        draft: {
          kind: "publish_market",
          command: {
            requestId: crypto.randomUUID(),
            printingId: draft.card.printingId,
            probeId,
            targetUsd: canonicalTarget(draft.targetUsd),
            condition: draft.condition,
            ...publicationTimes(draft.deadline, draft.schedule),
          },
        },
      },
      { onSuccess: settle, onError: () => settle(null) },
    );
  }, [publishTarget, journal, run, settle, probeId, pricePrinting]);

  const queries = useQueryClient();
  const [betTarget, setBetTarget] = useState<{
    predictionId: string;
    amount: number;
  } | null>(null);
  const [betPending, setBetPending] = useState(false);

  const quoteFor = useCallback(
    (predictionId: string | null, amount: number | null) => {
      if (!predictionId || amount === null || amount <= 0) {
        setBetTarget(null);
      } else {
        setBetTarget({ predictionId, amount });
      }
    },
    [],
  );

  const quote: ReadyPredictions["quote"] = useMemo(() => {
    if (!betTarget) return null;
    const market = allTerms.find((m) => m.id === betTarget.predictionId);
    if (!market || !market.acceptingOrders) {
      return {
        predictionId: betTarget.predictionId,
        amount: betTarget.amount,
        yes: { kind: "unavailable" },
        no: { kind: "unavailable" },
      };
    }
    // Whole-coin bet at 50/50 contract baseline (100 coins payout per contract).
    const contracts = Math.max(1, Math.floor(betTarget.amount / 50));
    const win = contracts * 100;
    const revision = `${betTarget.predictionId}:${betTarget.amount}:${contracts}`;
    return {
      predictionId: betTarget.predictionId,
      amount: betTarget.amount,
      yes: { kind: "ready", win, revision },
      no: { kind: "ready", win, revision },
    };
  }, [betTarget, allTerms]);

  const placeBet = useCallback(
    async (predictionId: string, side: "yes" | "no", amount: number) => {
      if (!owner || !services || betPending) return;
      setBetPending(true);
      setCommandFailure(null);
      try {
        const bearer = await ownerBearer(services, owner);
        const contracts = Math.max(1, Math.floor(amount / 50));
        const res = await services.api.predictionBet(bearer, predictionId, {
          requestId: crypto.randomUUID(),
          side,
          limitCostCoins: amount,
          quantity: contracts,
        });
        if (res.status === "success") {
          setCompleted({ kind: "bet" });
          await Promise.all([
            queries.invalidateQueries({
              queryKey: ["predictions", owner.accountId],
            }),
            queries.invalidateQueries({
              queryKey: ["account-summary", owner.accountId],
            }),
          ]);
        } else {
          setCommandFailure("Couldn’t place bet.");
        }
      } catch {
        setCommandFailure("Couldn’t reach the server.");
      } finally {
        setBetPending(false);
      }
    },
    [owner, services, betPending, queries],
  );

  if (gap) return { status: gap };
  if (markets.isPending) return { status: "loading" };
  const exploreFailed = markets.isError && !markets.data;
  const anyFailed =
    markets.isError || positions.isError || details.isError || labels.isError;
  const balance = Number(summary.data?.wallet.available);
  const price: PriceCheck | null = pricePrinting
    ? priceRead.data
      ? { printingId: pricePrinting, state: priceRead.data.state }
      : priceRead.isError
        ? { printingId: pricePrinting, state: "failed" }
        : { printingId: pricePrinting, state: "checking" }
    : null;
  return {
    status: "ready",
    balance: Number.isSafeInteger(balance) ? balance : null,
    explore: {
      items: explore,
      state: exploreFailed
        ? "failed"
        : labels.isPending && explore.length === 0
          ? "loading"
          : "ready",
      isAppending: markets.isFetchingNextPage,
      hasMore: !!markets.hasNextPage,
      loadMore: () => {
        if (markets.hasNextPage && !markets.isFetchingNextPage)
          void markets.fetchNextPage();
      },
    },
    mine: {
      bets: {
        items: bets,
        state:
          positions.isError && !positions.data
            ? "failed"
            : positionsComplete
              ? "ready"
              : "loading",
      },
      created: { items: [], state: "unavailable" },
    },
    failure:
      anyFailed && !isFailureDismissed
        ? exploreFailed
          ? LOAD_FAILED
          : UPDATE_FAILED
        : null,
    dismissFailure: () => setIsFailureDismissed(true),
    quoteFor,
    quote,
    betBlock: owner?.state === "active" ? null : "account",
    placeBet: (predictionId, side, amount) => {
      void placeBet(predictionId, side, amount);
    },
    betPending,
    searchCards,
    cards: {
      items: search.printings.items,
      state:
        cardQuery === null || !search.needle
          ? "idle"
          : search.cards.isError || search.printings.isError
            ? "failed"
            : search.cards.isPending ||
                (search.printings.isPending &&
                  search.printings.items.length === 0)
              ? "loading"
              : "ready",
      isAppending: search.cards.isFetchingNextPage,
      hasMore: !!search.cards.hasNextPage,
      loadMore: () => {
        if (search.cards.hasNextPage && !search.cards.isFetchingNextPage)
          void search.cards.fetchNextPage();
      },
    },
    checkPrice,
    price,
    publish: (draft) => {
      if (publishPending) return;
      setCommandFailure(null);
      setPublishPending(true);
      setPublishTarget({ draft, started: false });
    },
    publishPending,
    commandFailure,
    dismissCommandFailure: () => setCommandFailure(null),
    completed,
    dismissCompleted: () => setCompleted(null),
  };
}
