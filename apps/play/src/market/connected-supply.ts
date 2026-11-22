import { useCallback, useEffect, useMemo, useState } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import type { EconomyCommand } from "@spellwake/application-contracts";
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
import type {
  Completed,
  Purchase,
  PurchaseStage,
  QuoteTerms,
  SupplySource,
  SupplyTile,
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
 * same purchase, and a reload restores it without sending. The application
 * has no supply vote source yet, so the vote banner is not shown.
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
    ballot: null,
    saveBallot: () => undefined,
    checkBallot: () => undefined,
    dismissBallot: () => undefined,
    completed,
    dismissCompleted: () => setCompleted(null),
  };
}
