import { useCallback, useEffect, useMemo, useState } from "react";
import type { EconomyCommand } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { useAccountSummary } from "../data/account";
import { useEconomyCommand } from "../data/economy-commands";
import type { EconomyJournal } from "../data/economy-recovery";
import { useMarketListings, usePackProducts } from "../data/economy-queries";
import { finishLabel, RARITIES, type Rarity } from "../collection/card-sources";
import { usePageLabels } from "../collection/connected-cards";
import {
  useInventoryGap,
  useWholeInventory,
} from "../collection/connected-inventory";
import type {
  Completed,
  ListableAsset,
  Listing,
  ListingItem,
  ListingsSource,
  Operation,
  OperationStage,
} from "./listing-sources";

const LOAD_FAILED = "Couldn’t load listings.";
const UNAVAILABLE = "Listing no longer available.";
const ACCOUNT_LIMIT = "This purchase exceeds your account limit.";
const NO_COINS = "Not enough available coins.";
const NOT_ENOUGH = "Not enough available to list.";
const MARKET_CLOSED = "The market is unavailable for this account.";

const rarityOf = (rarity: string): Rarity | null =>
  (RARITIES as readonly string[]).includes(rarity) ? (rarity as Rarity) : null;
/** A wire amount as a number, or null when it is too large to show exactly. */
const exact = (amount: string): number | null => {
  const value = Number(amount);
  return Number.isSafeInteger(value) ? value : null;
};

type Kind = "listing_buy" | "listing_create" | "listing_cancel";
type Pending = { kind: Kind; targetId: string; operation: Operation };

/** The safe reason a known rejection is shown with. */
function rejection(journal: EconomyJournal, kind: Kind): string {
  if (journal.state !== "rejected") return UNAVAILABLE;
  const errors = journal.errors as Record<string, unknown>;
  if ("coins" in errors) return NO_COINS;
  if ("value" in errors) return ACCOUNT_LIMIT;
  if ("account" in errors) return MARKET_CLOSED;
  if ("assets" in errors && kind === "listing_create") return NOT_ENOUGH;
  return kind === "listing_create" ? "Couldn’t create listing." : UNAVAILABLE;
}

/** Commands of this account whose answer never came, as saved by their journals. Reading sends nothing. */
function unresolvedListingCommands(
  origin: string,
  accountId: string,
): EconomyCommand[] {
  const prefix = `spellwake.economy-command.v1:${origin}:${accountId}:listing_`;
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

/** Every page of one listings read. */
function useAllListings(owned: boolean) {
  const read = useMarketListings(owned ? { owned: true } : {});
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = read;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  return read;
}

/**
 * The account's Listings. Public listings and your own are read in full, so
 * search, filters and sorts apply to every listing; the service has no
 * listing time, so Newest is not offered. The catalogue names each card's
 * printing, finish and rarity and no more, so only Set, Rarity, Finish and
 * price filter cards. Your own are your active listings only. Buying,
 * creating and cancelling each send one journaled economy command; an
 * unknown outcome keeps its dialog and terms, and Check status asks about
 * that same attempt. A reload restores it without sending anything.
 */
export function useConnectedListings(): ListingsSource {
  const gap = useInventoryGap();
  const { state, services } = useAccount();
  const session =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  const summary = useAccountSummary();
  const publicRead = useAllListings(false);
  const ownRead = useAllListings(true);
  const inventory = useWholeInventory();
  const products = usePackProducts();
  // Every product page is read, so each pack has its name.
  const {
    hasNextPage: moreProducts,
    isFetchingNextPage: fetchingProducts,
    fetchNextPage: fetchProducts,
  } = products;
  useEffect(() => {
    if (moreProducts && !fetchingProducts) void fetchProducts();
  }, [moreProducts, fetchingProducts, fetchProducts]);

  const records = useMemo(() => {
    const byId = new Map<
      string,
      NonNullable<typeof publicRead.data>["pages"][number]["items"][number]
    >();
    for (const read of [publicRead.data, ownRead.data])
      for (const page of read?.pages ?? [])
        for (const item of page.items)
          if (item.state === "listed") byId.set(item.id, item);
    return [...byId.values()];
  }, [publicRead.data, ownRead.data]);

  // Names for every card printing listed or owned, read a hundred at a time.
  const printingPages = useMemo(() => {
    const ids = new Set<string>();
    for (const record of records)
      if (record.asset.kind === "card") ids.add(record.asset.printingId);
    for (const items of inventory.pages)
      for (const item of items)
        if (item.asset.kind === "card") ids.add(item.asset.printingId);
    const all = [...ids].sort();
    const pages: string[][] = [];
    for (let at = 0; at < all.length; at += 100)
      pages.push(all.slice(at, at + 100));
    return pages;
  }, [records, inventory.pages]);
  const labels = usePageLabels(printingPages);
  const named = useMemo(() => {
    const cards = new Map<string, string>();
    const printings = new Map<
      string,
      NonNullable<(typeof labels.pages)[number]>["printings"][number]
    >();
    for (const page of labels.pages) {
      for (const card of page?.cards ?? []) cards.set(card.id, card.name);
      for (const printing of page?.printings ?? [])
        printings.set(printing.id, printing);
    }
    return { cards, printings };
  }, [labels.pages]);
  const packNames = useMemo(
    () =>
      new Map(
        (products.data?.pages ?? []).flatMap((page) =>
          page.items.map((product) => [product.id, product] as const),
        ),
      ),
    [products.data],
  );

  /** A listed or owned asset as people read it, or null until its name is known. */
  const itemOf = useCallback(
    (
      asset:
        | { kind: "card"; printingId: string }
        | { kind: "pack"; recipeId: string }
        | { kind: "coin"; currencyKey: string },
    ): ListingItem | null => {
      if (asset.kind === "coin") return null;
      if (asset.kind === "pack") {
        const product = packNames.get(asset.recipeId);
        return {
          kind: "pack",
          product: product?.name ?? "Sealed pack",
          setName: product?.setName ?? "Sealed pack",
          setCode: null,
        };
      }
      const printing = named.printings.get(asset.printingId);
      const name = printing
        ? named.cards.get(printing.definitionId)
        : undefined;
      if (!printing || !name) return null;
      return {
        kind: "card",
        cardId: printing.definitionId,
        name,
        setCode: printing.set.code?.toUpperCase() ?? null,
        setName: printing.set.name,
        number: printing.collectorNumber,
        finish: finishLabel(printing.finish),
        rarity: rarityOf(printing.rarity),
        faceUrl: null,
        artUrl: null,
        type: null,
        rules: null,
        manaValue: null,
        colors: null,
        identity: null,
        legality: null,
      };
    },
    [named, packNames],
  );

  const accountId = session?.accountId ?? null;
  const listings = useMemo(
    () =>
      records.flatMap((record): Listing[] => {
        const item = itemOf(record.asset);
        const quantity = exact(record.quantity);
        if (!item || quantity === null) return [];
        return [
          {
            id: record.id,
            revision: record.revision,
            item,
            quantity,
            total: exact(record.totalPrice),
            totalText: record.totalPrice,
            seller: {
              id: record.seller.id,
              name:
                record.seller.id === accountId ? "You" : record.seller.username,
            },
            isMine: record.seller.id === accountId,
            listedAt: null,
          },
        ];
      }),
    [records, itemOf, accountId],
  );
  const assets = useMemo((): ListableAsset[] | null => {
    if (inventory.isPending) return null;
    return inventory.pages.flatMap((items) =>
      items.flatMap((entry): ListableAsset[] => {
        const item = itemOf(entry.asset);
        const available = exact(entry.available);
        if (!item || available === null || available < 1) return [];
        return [{ id: entry.assetId, item, available }];
      }),
    );
  }, [inventory.pages, inventory.isPending, itemOf]);

  // One command at a time, held open until its outcome is known.
  const [pending, setPending] = useState<Pending | null>(null);
  const [wantsSubmit, setWantsSubmit] = useState<EconomyCommand | null>(null);
  const [completed, setCompleted] = useState<Completed | null>(null);
  const [isFailureDismissed, setIsFailureDismissed] = useState(false);
  const command = useEconomyCommand(
    pending ? { kind: pending.kind, targetId: pending.targetId } : null,
  );
  const { journal, run } = command;

  const withStage = (current: Pending, stage: OperationStage): Pending => ({
    ...current,
    operation: { ...current.operation, stage },
  });
  const settle = useCallback((result: EconomyJournal | null) => {
    setPending((current) => {
      if (!current) return current;
      if (result?.state === "accepted") {
        const { operation } = current;
        setCompleted(
          operation.kind === "buy"
            ? { kind: "bought", listing: operation.listing }
            : operation.kind === "cancel"
              ? { kind: "cancelled", listingId: operation.listing.id }
              : { kind: "created", listingId: result.ack.resourceId },
        );
        return null;
      }
      if (result?.state === "rejected")
        return withStage(current, {
          kind: "rejected",
          message: rejection(result, current.kind),
        });
      return withStage(current, { kind: "unconfirmed", isChecking: false });
    });
  }, []);

  // The first attempt is sent once its journal is in hand; an earlier unanswered one is settled first.
  useEffect(() => {
    if (!wantsSubmit || !pending) return;
    setWantsSubmit(null);
    if (journal.status === "unavailable")
      return setPending((current) =>
        current
          ? withStage(current, { kind: "rejected", message: MARKET_CLOSED })
          : current,
      );
    const prior = journal.status === "saved" ? journal.value : null;
    if (prior?.state === "unresolved")
      return setPending((current) =>
        current
          ? withStage(current, { kind: "unconfirmed", isChecking: false })
          : current,
      );
    run.mutate(
      { type: "submit", expected: prior, draft: wantsSubmit },
      { onSuccess: settle, onError: () => settle(null) },
    );
  }, [wantsSubmit, pending, journal, run, settle]);

  // A reload restores a command that never got its answer, and sends nothing.
  const origin = services?.api.origin ?? null;
  useEffect(() => {
    if (!origin || !accountId || pending) return;
    for (const intent of unresolvedListingCommands(origin, accountId)) {
      if (intent.kind === "listing_buy" || intent.kind === "listing_cancel") {
        const listing = listings.find(
          (entry) => entry.id === intent.command.listingId,
        );
        if (!listing) continue;
        setPending({
          kind: intent.kind,
          targetId: listing.id,
          operation: {
            kind: intent.kind === "listing_buy" ? "buy" : "cancel",
            listing,
            stage: { kind: "unconfirmed", isChecking: false },
          },
        });
        return;
      }
      if (intent.kind === "listing_create") {
        const asset = (assets ?? []).find(
          (entry) => entry.id === intent.command.lot.assetId,
        );
        const quantity = exact(intent.command.lot.quantity);
        const total = exact(intent.command.totalPrice);
        if (!asset || quantity === null || total === null) continue;
        setPending({
          kind: "listing_create",
          targetId: asset.id,
          operation: {
            kind: "create",
            asset,
            quantity,
            total,
            stage: { kind: "unconfirmed", isChecking: false },
          },
        });
        return;
      }
    }
  }, [origin, accountId, pending, listings, assets]);

  const isBusy =
    pending !== null && pending.operation.stage.kind !== "rejected";
  const start = (next: Pending, draft: EconomyCommand) => {
    if (isBusy) return;
    setPending(next);
    setWantsSubmit(draft);
  };

  if (gap) return { status: gap };
  if (publicRead.isPending && ownRead.isPending) return { status: "loading" };
  const readFailed =
    (publicRead.isError && !publicRead.data) ||
    (ownRead.isError && !ownRead.data);
  const isUnread = readFailed && listings.length === 0;
  const balance = Number(summary.data?.wallet.available);
  return {
    status: "ready",
    listings,
    facets: {
      formats: null,
      color: false,
      type: false,
      rarity: true,
      manaValue: false,
      sorts: ["price-asc", "price-desc", "name-asc"],
    },
    isAppending:
      publicRead.hasNextPage ||
      ownRead.hasNextPage ||
      labels.isPending ||
      publicRead.isFetchingNextPage,
    isUnread,
    failure:
      (readFailed || labels.isError) && !isFailureDismissed
        ? LOAD_FAILED
        : null,
    dismissFailure: () => setIsFailureDismissed(true),
    balance: Number.isSafeInteger(balance) ? balance : null,
    assets,
    canTrade: session?.state === "active",
    operation: pending?.operation ?? null,
    completed,
    dismissCompleted: () => setCompleted(null),
    buy: (listingId) => {
      const listing = listings.find((entry) => entry.id === listingId);
      if (!listing || listing.isMine || listing.total === null) return;
      start(
        {
          kind: "listing_buy",
          targetId: listing.id,
          operation: { kind: "buy", listing, stage: { kind: "pending" } },
        },
        {
          kind: "listing_buy",
          command: {
            requestId: crypto.randomUUID(),
            listingId: listing.id,
            expectedRevision: listing.revision,
            expectedTotal: listing.totalText,
          },
        },
      );
    },
    create: (assetId, quantity, total) => {
      const asset = assets?.find((entry) => entry.id === assetId);
      if (!asset) return;
      start(
        {
          kind: "listing_create",
          targetId: asset.id,
          operation: {
            kind: "create",
            asset,
            quantity,
            total,
            stage: { kind: "pending" },
          },
        },
        {
          kind: "listing_create",
          command: {
            requestId: crypto.randomUUID(),
            lot: { assetId: asset.id, quantity: String(quantity) },
            totalPrice: String(total),
          },
        },
      );
    },
    cancel: (listingId) => {
      const listing = listings.find((entry) => entry.id === listingId);
      if (!listing?.isMine) return;
      start(
        {
          kind: "listing_cancel",
          targetId: listing.id,
          operation: { kind: "cancel", listing, stage: { kind: "pending" } },
        },
        {
          kind: "listing_cancel",
          command: { requestId: crypto.randomUUID(), listingId: listing.id },
        },
      );
    },
    checkStatus: () => {
      if (!pending || pending.operation.stage.kind !== "unconfirmed") return;
      if (journal.status !== "saved" || journal.value.state !== "unresolved") {
        settle(journal.status === "saved" ? journal.value : null);
        return;
      }
      setPending(withStage(pending, { kind: "unconfirmed", isChecking: true }));
      run.mutate(
        { type: "recover", expected: journal.value },
        { onSuccess: settle, onError: () => settle(null) },
      );
    },
    dismissOperation: () =>
      setPending((current) =>
        current?.operation.stage.kind === "rejected" ? null : current,
      ),
  };
}
