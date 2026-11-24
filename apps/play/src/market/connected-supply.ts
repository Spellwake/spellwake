import { useCallback, useEffect, useMemo, useState } from "react";
import {
  useInfiniteQuery,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  ballotPageSchema,
  type BallotPage,
  type EconomyCommand,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer, useAccountSummary } from "../data/account";
import { useEconomyCommand } from "../data/economy-commands";
import type { EconomyJournal } from "../data/economy-recovery";
import {
  usePackProducts,
  usePackQuote,
  usePackQuotes,
} from "../data/economy-queries";
import {
  READ_ATTEMPTS,
  useInventoryGap,
} from "../collection/connected-inventory";
import {
  ballotDates,
  shortDate,
  type Ballot,
  type BallotSave,
  type Completed,
  type Purchase,
  type PurchaseStage,
  type QuoteTerms,
  type SupplySource,
  type SupplyTile,
} from "./supply-sources";

const LOAD_FAILED = "Couldn’t load sets.";
const NO_COINS = "Not enough available coins.";
const CHANGED = "Price changed. Check the new total.";
const GONE = "Product no longer available.";
const CLOSED = "Buying is unavailable.";

/** A wire amount as a number, or null when it is too large to show exactly. */
const exact = (amount: string | null): number | null => {
  if (amount === null) return null;
  const value = Number(amount);
  return Number.isSafeInteger(value) ? value : null;
};
type WireQuote = {
  quantity: string;
  remaining: string;
  total: string | null;
  policyId: string;
  revision: string;
};
const termsOf = (quote: WireQuote): QuoteTerms | null => {
  const remaining = exact(quote.remaining);
  const quantity = exact(quote.quantity);
  if (remaining === null || quantity === null) return null;
  return {
    quantity,
    remaining,
    total: exact(quote.total),
    policyId: quote.policyId,
    revision: quote.revision,
    totalText: quote.total,
  };
};

function rejection(journal: EconomyJournal): string {
  if (journal.state !== "rejected") return GONE;
  const errors = journal.errors as Record<string, unknown>;
  if ("coins" in errors) return NO_COINS;
  if ("quote" in errors) return CHANGED;
  if ("account" in errors) return CLOSED;
  return GONE;
}

/** Purchases this account sent whose answer never came. Reading sends nothing. */
function unresolvedPurchases(
  origin: string,
  accountId: string,
): EconomyCommand[] {
  const prefix = `spellwake.economy-command.v1:${origin}:${accountId}:pack_buy:`;
  const found: EconomyCommand[] = [];
  try {
    for (let index = 0; index < window.localStorage.length; index += 1) {
      const key = window.localStorage.key(index);
      if (!key?.startsWith(prefix)) continue;
      const value = JSON.parse(window.localStorage.getItem(key) ?? "null") as {
        state?: string;
        command?: { intent?: EconomyCommand };
      } | null;
      if (value?.state === "unresolved" && value.command?.intent)
        found.push(value.command.intent);
    }
  } catch {
    // Unreadable storage restores nothing; the journal still guards any new send.
  }
  return found;
}

/** Every published set, read in full so search covers the whole catalogue. */
function useAllSets() {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const read = useInfiniteQuery({
    queryKey: ["supply-sets", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    initialPageParam: null as string | null,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ pageParam, signal }) => {
      if (!owner || !services) throw new Error("Sign in to see sets.");
      const result = await services.api.sets(
        await ownerBearer(services, owner),
        "",
        pageParam,
        signal,
      );
      if (result.status !== "success") throw new Error(LOAD_FAILED);
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
  });
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = read;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  return read;
}

/**
 * The account's Supply. Every set and every qualified pack product are read
 * in full and joined by set, so search and Availability cover the whole
 * catalogue; each product's quantity-one quote says whether it is available,
 * sold out or not yet known. Buy packs quotes the chosen quantity and sends
 * one journaled `pack_buy` naming that quote's policy, revision and exact
 * total; an unknown outcome keeps the dialog and Check status recovers that
 * same purchase, and a reload restores it without sending. The vote banner
 * shows while an active season has a supply ballot, read and saved whole.
 */
export function useConnectedSupply(): SupplySource {
  const gap = useInventoryGap();
  const { state, services } = useAccount();
  const session =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const summary = useAccountSummary();
  const sets = useAllSets();
  const products = usePackProducts();
  const {
    hasNextPage: moreProducts,
    isFetchingNextPage: fetchingProducts,
    fetchNextPage: fetchProducts,
  } = products;
  useEffect(() => {
    if (moreProducts && !fetchingProducts) void fetchProducts();
  }, [moreProducts, fetchingProducts, fetchProducts]);

  const productList = useMemo(
    () => (products.data?.pages ?? []).flatMap((page) => page.items),
    [products.data],
  );
  const quotes = usePackQuotes(productList.map((item) => item.id));
  const quoteKey = quotes
    .map((quote) => `${quote.status}:${quote.dataUpdatedAt}`)
    .join("|");
  const tiles = useMemo((): SupplyTile[] => {
    const setList = (sets.data?.pages ?? []).flatMap((page) => page.items);
    const offered = new Set(productList.map((item) => item.setId));
    const productTiles = productList.map((item, index): SupplyTile => {
      const set = setList.find((entry) => entry.id === item.setId);
      const read = quotes[index];
      const terms = read?.data ? termsOf(read.data.quote) : null;
      return {
        kind: "product",
        id: item.id,
        setId: item.setId,
        setName: item.setName,
        setCode: set?.code?.toUpperCase() ?? null,
        product: item.name.startsWith(item.setName)
          ? item.name.slice(item.setName.length).trim() || item.name
          : item.name,
        fullName: item.name,
        quote: terms,
        quoteState: terms ? "ready" : read?.isError ? "failed" : "checking",
      };
    });
    const bare = setList
      .filter((set) => !offered.has(set.id))
      .map((set): SupplyTile => ({
        kind: "set",
        id: set.id,
        setName: set.name,
        setCode: set.code?.toUpperCase() ?? null,
      }));
    return [...productTiles, ...bare];
    // Quotes are compared by their read state, as the list of queries is rebuilt each render.
  }, [sets.data, productList, quoteKey]);

  // The open dialog's quote, for its product and quantity.
  const [target, setTarget] = useState<{
    tileId: string;
    quantity: number;
  } | null>(null);
  const quoteFor = useCallback(
    (tileId: string | null, quantity: number) =>
      setTarget((current) =>
        tileId === null
          ? null
          : current?.tileId === tileId && current.quantity === quantity
            ? current
            : { tileId, quantity },
      ),
    [],
  );
  const dialogQuote = usePackQuote(
    target?.tileId ?? null,
    String(target?.quantity ?? 1),
    target !== null,
  );

  // One purchase at a time, held open until its outcome is known.
  const [purchase, setPurchase] = useState<Purchase | null>(null);
  const [wantsSubmit, setWantsSubmit] = useState<EconomyCommand | null>(null);
  const [completed, setCompleted] = useState<Completed | null>(null);
  const [isFailureDismissed, setIsFailureDismissed] = useState(false);
  const command = useEconomyCommand(
    purchase ? { kind: "pack_buy", targetId: purchase.tileId } : null,
  );
  const { journal, run } = command;
  const withStage = (current: Purchase, stage: PurchaseStage): Purchase => ({
    ...current,
    stage,
  });
  const settle = useCallback(
    (result: EconomyJournal | null) =>
      setPurchase((current) => {
        if (!current) return current;
        if (result?.state === "accepted") {
          const item = productList.find((entry) => entry.id === current.tileId);
          setCompleted({
            kind: "bought",
            setName: item?.setName ?? "sealed",
            quantity: current.quantity,
          });
          return null;
        }
        if (result?.state === "rejected")
          return withStage(current, {
            kind: "rejected",
            message: rejection(result),
          });
        return withStage(current, { kind: "unconfirmed", isChecking: false });
      }),
    [productList],
  );
  useEffect(() => {
    if (!wantsSubmit || !purchase) return;
    setWantsSubmit(null);
    if (journal.status === "unavailable")
      return setPurchase((current) =>
        current
          ? withStage(current, { kind: "rejected", message: CLOSED })
          : current,
      );
    const prior = journal.status === "saved" ? journal.value : null;
    if (prior?.state === "unresolved")
      return setPurchase((current) =>
        current
          ? withStage(current, { kind: "unconfirmed", isChecking: false })
          : current,
      );
    run.mutate(
      { type: "submit", expected: prior, draft: wantsSubmit },
      { onSuccess: settle, onError: () => settle(null) },
    );
  }, [wantsSubmit, purchase, journal, run, settle]);

  // A reload restores a purchase that never got its answer, and sends nothing.
  const origin = services?.api.origin ?? null;
  const accountId = session?.accountId ?? null;
  useEffect(() => {
    if (!origin || !accountId || purchase) return;
    for (const intent of unresolvedPurchases(origin, accountId)) {
      if (intent.kind !== "pack_buy") continue;
      const {
        recipeId,
        quantity,
        expectedPolicyId,
        expectedRevision,
        expectedTotal,
      } = intent.command;
      const count = exact(quantity);
      const total = exact(expectedTotal);
      if (count === null || total === null) continue;
      setPurchase({
        tileId: recipeId,
        quantity: count,
        terms: {
          quantity: count,
          remaining: count,
          total,
          policyId: expectedPolicyId,
          revision: expectedRevision,
          totalText: expectedTotal,
        },
        stage: { kind: "unconfirmed", isChecking: false },
      });
      return;
    }
  }, [origin, accountId, purchase]);

  const queries = useQueryClient();
  const [ballotSave, setBallotSave] = useState<BallotSave | null>(null);

  // Read active seasons and their supply ballot when available.
  const ballotQuery = useQuery({
    queryKey: ["supply-ballot", session?.accountId, session?.sessionId],
    enabled: !!session && !!services,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({
      signal,
    }): Promise<{
      seasonId: string;
      page: BallotPage;
    } | null> => {
      if (!session || !services) return null;
      const bearer = await ownerBearer(services, session);
      const seasonsRes = await services.api.seasons(bearer, null, 20, signal);
      if (seasonsRes.status !== "success") return null;
      const rawItems = Array.isArray(seasonsRes.data["items"])
        ? (seasonsRes.data["items"] as Array<Record<string, unknown>>)
        : [];
      const active =
        rawItems.find((s) => s["state"] === "active") ?? rawItems[0];
      const seasonId =
        active && typeof active["id"] === "string" ? active["id"] : null;
      if (!seasonId) return null;
      const ballotRes = await services.api.seasonBallot(
        bearer,
        seasonId,
        signal,
      );
      if (ballotRes.status !== "success") return null;
      const parsed = ballotPageSchema.safeParse(ballotRes.data);
      return parsed.success ? { seasonId, page: parsed.data } : null;
    },
  });

  const setCodes = useMemo(
    () =>
      new Map(
        (sets.data?.pages ?? []).flatMap((page) =>
          page.items.map(
            (set) => [set.id, set.code?.toUpperCase() ?? null] as const,
          ),
        ),
      ),
    [sets.data],
  );

  const ballot: Ballot | null = useMemo(() => {
    if (!ballotQuery.data) return null;
    const { page } = ballotQuery.data;
    const dates = ballotDates(new Date(page.cutoff));
    const closes = shortDate(new Date(page.cutoff));
    const nextDrop = shortDate(dates.nextDrop);
    const winners = page.items.filter((c) => c.winner === true);
    return {
      status: "ready",
      closes,
      nextDrop,
      isOpen: page.isOpen,
      canVote: page.canVote,
      qualifies: page.qualifies,
      candidates: page.items.map((item) => ({
        setId: item.setId,
        name: item.name,
        code: setCodes.get(item.setId) ?? null,
      })),
      saved: page.approvedSetIds,
      results:
        page.publication !== null
          ? winners.map((w) => ({
              name: w.name,
              votes: Number(w.finalApprovals ?? 0),
              packs: Number(w.dropQuantity ?? 0),
            }))
          : null,
      readFailed: ballotQuery.isError,
      save: ballotSave,
    };
  }, [ballotQuery.data, ballotQuery.isError, setCodes, ballotSave]);

  const saveBallot = useCallback(
    async (setIds: readonly string[]) => {
      if (!session || !services || !ballotQuery.data) return;
      setBallotSave({ kind: "pending" });
      try {
        const bearer = await ownerBearer(services, session);
        const res = await services.api.replaceSeasonBallot(
          bearer,
          ballotQuery.data.seasonId,
          { requestId: crypto.randomUUID(), setIds: [...setIds] },
        );
        if (res.status === "success") {
          setBallotSave(null);
          setCompleted({ kind: "voted" });
          await queries.invalidateQueries({
            queryKey: ["supply-ballot", session.accountId],
          });
        } else if (res.status === "fail") {
          setBallotSave({
            kind: "rejected",
            message: "Couldn’t save vote. Check your eligibility and choices.",
          });
        } else {
          setBallotSave({ kind: "unconfirmed", isChecking: false });
        }
      } catch {
        setBallotSave({ kind: "unconfirmed", isChecking: false });
      }
    },
    [session, services, ballotQuery.data, queries],
  );

  const checkBallot = useCallback(async () => {
    setBallotSave({ kind: "unconfirmed", isChecking: true });
    const fresh = await ballotQuery.refetch();
    setBallotSave(
      fresh.data ? null : { kind: "unconfirmed", isChecking: false },
    );
  }, [ballotQuery]);

  if (gap) return { status: gap };
  if (sets.isPending && products.isPending) return { status: "loading" };
  const readFailed =
    (sets.isError && !sets.data) || (products.isError && !products.data);
  const balance = Number(summary.data?.wallet.available);
  const dialogTerms = dialogQuote.data ? termsOf(dialogQuote.data.quote) : null;
  return {
    status: "ready",
    tiles,
    isAppending:
      sets.hasNextPage || products.hasNextPage || sets.isFetchingNextPage,
    isUnread: readFailed && tiles.length === 0,
    failure: readFailed && !isFailureDismissed ? LOAD_FAILED : null,
    dismissFailure: () => setIsFailureDismissed(true),
    balance: Number.isSafeInteger(balance) ? balance : null,
    canBuy: session?.state === "active",
    quote: target
      ? {
          ...target,
          state: dialogQuote.isError
            ? "failed"
            : dialogTerms
              ? "ready"
              : "loading",
          terms: dialogTerms,
        }
      : null,
    quoteFor,
    purchase,
    buy: (tileId, quantity, terms) => {
      if (terms.total === null || terms.totalText === null) return;
      if (purchase && purchase.stage.kind !== "rejected") return;
      setPurchase({ tileId, quantity, terms, stage: { kind: "pending" } });
      setWantsSubmit({
        kind: "pack_buy",
        command: {
          requestId: crypto.randomUUID(),
          recipeId: tileId,
          expectedPolicyId: terms.policyId,
          quantity: String(quantity),
          expectedRevision: terms.revision,
          expectedTotal: terms.totalText,
        },
      });
    },
    checkPurchase: () => {
      if (!purchase || purchase.stage.kind !== "unconfirmed") return;
      if (journal.status !== "saved" || journal.value.state !== "unresolved") {
        settle(journal.status === "saved" ? journal.value : null);
        return;
      }
      setPurchase(
        withStage(purchase, { kind: "unconfirmed", isChecking: true }),
      );
      run.mutate(
        { type: "recover", expected: journal.value },
        { onSuccess: settle, onError: () => settle(null) },
      );
    },
    dismissPurchase: () =>
      setPurchase((current) =>
        current?.stage.kind === "rejected" ? null : current,
      ),
    ballot,
    saveBallot: (ids) => void saveBallot(ids),
    checkBallot: () => void checkBallot(),
    dismissBallot: () => setBallotSave(null),
    completed,
    dismissCompleted: () => setCompleted(null),
  };
}
