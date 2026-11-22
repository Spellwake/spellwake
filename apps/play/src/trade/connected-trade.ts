import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  identifierSchema,
  type TradeSession,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { usePageLabels } from "../collection/connected-cards";
import { useWholeInventory } from "../collection/connected-inventory";
import { ownerBearer, useAccountSummary } from "../data/account";
import { scryfallImage } from "../data/scryfall-images";
import {
  capQuantity,
  COINS_ID,
  inTrayOrder,
  type OfferLine,
  type Readiness,
  type TradeAsset,
  type TradeHolding,
  type TradePending,
  type TradeSource,
  type TradeStage,
} from "./trade-sources";

const POLL_MS = 2000;
const COIN_ASSET: TradeAsset = {
  id: COINS_ID,
  kind: "coin",
  name: "Coins",
  detail: null,
  artUrl: null,
  setName: null,
};

function readinessOf(ready: boolean, confirmed: boolean): Readiness {
  if (confirmed) return "confirmed";
  if (ready) return "ready";
  return "editing";
}

function assetFromSessionItem(
  item: TradeSession["initiatorItems"][number],
  knownHoldings: ReadonlyMap<string, TradeHolding>,
): OfferLine {
  const existing = knownHoldings.get(item.assetId);
  if (existing) {
    return {
      id: existing.id,
      kind: existing.kind,
      name: existing.name,
      detail: existing.detail,
      artUrl: existing.artUrl,
      setName: existing.setName,
      quantity: Number(item.quantity) || 0,
    };
  }
  const raw = item.asset;
  const kindRaw = typeof raw["kind"] === "string" ? raw["kind"] : "card";
  const kind = kindRaw === "pack" ? "pack" : "card";
  const name =
    typeof raw["name"] === "string" && raw["name"].trim()
      ? raw["name"]
      : kind === "pack"
        ? "Booster pack"
        : "Card";
  const setCode =
    typeof raw["setCode"] === "string" ? raw["setCode"].toUpperCase() : null;
  const finish = typeof raw["finish"] === "string" ? raw["finish"] : null;
  const extRef =
    typeof raw["externalPrintingRef"] === "string"
      ? raw["externalPrintingRef"]
      : null;
  return {
    id: item.assetId,
    kind,
    name,
    detail:
      kind === "card"
        ? [setCode, finish].filter(Boolean).join(" · ") || null
        : null,
    artUrl: extRef ? scryfallImage(extRef, "art_crop") : null,
    setName: typeof raw["setName"] === "string" ? raw["setName"] : null,
    quantity: Number(item.quantity) || 0,
  };
}

function linesFromSide(
  items: TradeSession["initiatorItems"],
  coinsRaw: string,
  holdingsMap: ReadonlyMap<string, TradeHolding>,
): OfferLine[] {
  const lines = items
    .map((item) => assetFromSessionItem(item, holdingsMap))
    .filter((line) => line.quantity > 0);
  const coins = Number(coinsRaw) || 0;
  if (coins > 0) {
    lines.push({ ...COIN_ASSET, quantity: coins });
  }
  return inTrayOrder(lines);
}

/**
 * Bilateral live trade room backed by `/v1/trades/sessions/*`.
 * Finds or opens an active session with `partnerId`, polls every 2 seconds
 * while active, synchronizes offer edits, readiness, confirmation, and
 * cancellation against the session revision.
 */
export function useConnectedTrade(partnerId: string): TradeSource {
  const { state, services } = useAccount();
  const queries = useQueryClient();
  const summary = useAccountSummary();
  const inventory = useWholeInventory();
  const isValidPartner = identifierSchema.safeParse(partnerId).success;
  const owner =
    state.status === "ready" && state.session.accountId
      ? { ...state.session, accountId: state.session.accountId }
      : null;

  const [sessionId, setSessionId] = useState<string | null>(null);
  const [session, setSession] = useState<TradeSession | null>(null);
  const [initState, setInitState] = useState<"idle" | "loading" | "failed">(
    "idle",
  );
  const [pending, setPending] = useState<TradePending>(null);
  const [unconfirmed, setUnconfirmed] = useState<boolean>(false);
  const [isChecking, setIsChecking] = useState<boolean>(false);
  const [notice, setNotice] = useState<string | null>(null);
  const creatingRef = useRef(false);

  // Locate an existing active session with this partner, or create one.
  useEffect(() => {
    if (!owner || !services || !isValidPartner || creatingRef.current) return;
    if (sessionId) return;
    creatingRef.current = true;
    setInitState("loading");
    let cancelled = false;
    (async () => {
      try {
        const bearer = await ownerBearer(services, owner);
        const existing = await services.api.tradeSessions(bearer, {
          state: "active",
          limit: 50,
        });
        if (!cancelled && existing.status === "success") {
          const match = existing.data.items.find(
            (s) =>
              (s.initiator.id === owner.accountId &&
                s.counterparty.id === partnerId) ||
              (s.counterparty.id === owner.accountId &&
                s.initiator.id === partnerId),
          );
          if (match) {
            setSessionId(match.id);
            setSession(match);
            setInitState("idle");
            return;
          }
        }
        const created = await services.api.createTradeSession(
          bearer,
          partnerId,
          crypto.randomUUID(),
        );
        if (!cancelled) {
          if (created.status === "success") {
            setSessionId(created.data.session.id);
            setSession(created.data.session);
            setInitState("idle");
          } else {
            setInitState("failed");
          }
        }
      } catch {
        if (!cancelled) setInitState("failed");
      } finally {
        creatingRef.current = false;
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [owner, services, isValidPartner, partnerId, sessionId]);

  // Poll the active session every 2s for counterparty updates.
  const detailQuery = useQuery({
    queryKey: ["trade-session", owner?.accountId, sessionId],
    enabled: !!owner && !!services && !!sessionId,
    refetchInterval: session?.state === "active" ? POLL_MS : false,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !sessionId) return null;
      const res = await services.api.tradeSessionDetail(
        await ownerBearer(services, owner),
        sessionId,
        signal,
      );
      if (res.status !== "success") throw new Error("Trade unavailable.");
      return res.data.session;
    },
  });

  useEffect(() => {
    if (detailQuery.data) {
      setSession(detailQuery.data);
      if (detailQuery.data.state !== "active") setUnconfirmed(false);
    }
  }, [detailQuery.data]);

  // Resolve inventory labels for Add items picker.
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

  const holdings = useMemo((): TradeHolding[] => {
    const cardNames = new Map<string, string>();
    const printings = new Map<
      string,
      {
        definitionId: string;
        setCode: string | null;
        setName: string;
        finish: string;
        externalPrintingRef: string;
      }
    >();
    for (const page of labels.pages) {
      for (const card of page?.cards ?? []) cardNames.set(card.id, card.name);
      for (const p of page?.printings ?? []) {
        printings.set(p.id, {
          definitionId: p.definitionId,
          setCode: p.set.code?.toUpperCase() ?? null,
          setName: p.set.name,
          finish: p.finish,
          externalPrintingRef: p.externalPrintingRef,
        });
      }
    }
    const list: TradeHolding[] = [];
    for (const items of inventory.pages) {
      for (const item of items) {
        const owned = Number(item.available) || 0;
        if (owned <= 0) continue;
        if (item.asset.kind === "card") {
          const p = printings.get(item.asset.printingId);
          const name = p ? cardNames.get(p.definitionId) : undefined;
          if (!p || !name) continue;
          list.push({
            id: item.assetId,
            kind: "card",
            name,
            detail:
              [p.setCode, p.finish === "foil" ? "Foil" : null]
                .filter(Boolean)
                .join(" · ") || null,
            artUrl: scryfallImage(p.externalPrintingRef, "art_crop"),
            setName: p.setName,
            owned,
          });
        } else if (item.asset.kind === "pack") {
          list.push({
            id: item.assetId,
            kind: "pack",
            name: "Booster pack",
            detail: null,
            artUrl: null,
            setName: null,
            owned,
          });
        }
      }
    }
    const coins = Number(summary.data?.wallet.available);
    if (Number.isSafeInteger(coins) && coins > 0) {
      list.push({ ...COIN_ASSET, owned: coins });
    }
    return list;
  }, [inventory.pages, labels.pages, summary.data?.wallet.available]);

  const holdingsMap = useMemo(
    () => new Map(holdings.map((h) => [h.id, h] as const)),
    [holdings],
  );

  const isInitiator =
    session && owner ? session.initiator.id === owner.accountId : true;

  const ownLines = useMemo(
    () =>
      session
        ? linesFromSide(
            isInitiator ? session.initiatorItems : session.counterpartyItems,
            isInitiator ? session.initiatorCoins : session.counterpartyCoins,
            holdingsMap,
          )
        : [],
    [session, isInitiator, holdingsMap],
  );

  const otherLines = useMemo(
    () =>
      session
        ? linesFromSide(
            isInitiator ? session.counterpartyItems : session.initiatorItems,
            isInitiator ? session.counterpartyCoins : session.initiatorCoins,
            holdingsMap,
          )
        : [],
    [session, isInitiator, holdingsMap],
  );

  const pushOffer = useCallback(
    async (nextLines: OfferLine[]) => {
      if (!owner || !services || !session || session.state !== "active") return;
      const coins = nextLines.find((l) => l.kind === "coin")?.quantity ?? 0;
      const items = nextLines
        .filter((l) => l.kind !== "coin" && l.quantity > 0)
        .map((l) => ({ assetId: l.id, quantity: l.quantity }));
      try {
        const bearer = await ownerBearer(services, owner);
        const res = await services.api.updateTradeSessionOffer(
          bearer,
          session.id,
          {
            expectedRevision: Number(session.revision),
            coins,
            items,
          },
        );
        if (res.status === "success") {
          setSession(res.data.session);
          setNotice(null);
        } else {
          setNotice("Couldn’t update offer.");
          void detailQuery.refetch();
        }
      } catch {
        setNotice("Couldn’t reach the server.");
      }
    },
    [owner, services, session, detailQuery],
  );

  const setQuantity = useCallback(
    (assetId: string, quantity: number) => {
      const held = holdingsMap.get(assetId);
      const capped = held
        ? capQuantity(quantity, held.owned)
        : Math.max(0, Math.floor(quantity));
      const exists = ownLines.some((l) => l.id === assetId);
      const next = exists
        ? ownLines
            .map((l) => (l.id === assetId ? { ...l, quantity: capped } : l))
            .filter((l) => l.quantity > 0)
        : capped > 0 && held
          ? [...ownLines, { ...held, quantity: capped }]
          : ownLines;
      void pushOffer(next);
    },
    [holdingsMap, ownLines, pushOffer],
  );

  const add = useCallback(
    (assetId: string) => {
      const current = ownLines.find((l) => l.id === assetId)?.quantity ?? 0;
      setQuantity(assetId, current + 1);
    },
    [ownLines, setQuantity],
  );

  const setReadyFlag = useCallback(
    async (ready: boolean, kind: "ready" | "edit") => {
      if (!owner || !services || !session || pending) return;
      setPending(kind);
      try {
        const bearer = await ownerBearer(services, owner);
        const res = await services.api.setTradeSessionReady(
          bearer,
          session.id,
          Number(session.revision),
          ready,
        );
        if (res.status === "success") {
          setSession(res.data.session);
          setNotice(null);
        } else {
          setNotice(
            ready
              ? "Couldn’t mark ready. Check your items."
              : "Couldn’t reopen items for editing.",
          );
          void detailQuery.refetch();
        }
      } catch {
        setNotice("Couldn’t reach the server.");
      } finally {
        setPending(null);
      }
    },
    [owner, services, session, pending, detailQuery],
  );

  const trade = useCallback(async () => {
    if (!owner || !services || !session || pending) return;
    setPending("trade");
    try {
      const bearer = await ownerBearer(services, owner);
      const res = await services.api.confirmTradeSession(
        bearer,
        session.id,
        Number(session.revision),
      );
      if (res.status === "success") {
        setSession(res.data.session);
        setNotice(null);
        if (res.data.session.state === "completed") {
          await Promise.all([
            queries.invalidateQueries({
              queryKey: ["account-summary", owner.accountId],
            }),
            queries.invalidateQueries({
              queryKey: ["inventory", owner.accountId],
            }),
          ]);
        }
      } else if (res.status === "fail") {
        setNotice("Trade terms changed. Review and confirm again.");
        void detailQuery.refetch();
      } else {
        setUnconfirmed(true);
      }
    } catch {
      setUnconfirmed(true);
    } finally {
      setPending(null);
    }
  }, [owner, services, session, pending, queries, detailQuery]);

  const cancel = useCallback(async () => {
    if (!owner || !services || !session || pending) return;
    setPending("cancel");
    try {
      const bearer = await ownerBearer(services, owner);
      const res = await services.api.cancelTradeSession(bearer, session.id);
      if (res.status === "success") {
        setSession(res.data.session);
        setNotice(null);
      } else {
        setNotice("Couldn’t cancel trade.");
      }
    } catch {
      setNotice("Couldn’t reach the server.");
    } finally {
      setPending(null);
    }
  }, [owner, services, session, pending]);

  const checkStatus = useCallback(async () => {
    setIsChecking(true);
    try {
      const fresh = await detailQuery.refetch();
      if (fresh.data) {
        setSession(fresh.data);
        setUnconfirmed(false);
      }
    } finally {
      setIsChecking(false);
    }
  }, [detailQuery]);

  if (state.status === "signed_out") return { status: "signed-out" };
  if (state.status !== "ready") return { status: "loading" };
  if (state.session.state === "needs_registration")
    return { status: "unregistered" };
  if (!isValidPartner || initState === "failed")
    return { status: "unavailable" };
  if (!session) return { status: "loading" };

  const partner = isInitiator ? session.counterparty : session.initiator;
  const ownStatus = readinessOf(
    isInitiator ? session.initiatorReady : session.counterpartyReady,
    isInitiator ? session.initiatorConfirmed : session.counterpartyConfirmed,
  );
  const otherStatus = readinessOf(
    isInitiator ? session.counterpartyReady : session.initiatorReady,
    isInitiator ? session.counterpartyConfirmed : session.initiatorConfirmed,
  );

  const stage: TradeStage =
    session.state === "completed"
      ? { kind: "complete", gave: ownLines, received: otherLines }
      : session.state === "canceled"
        ? { kind: "closed", reason: "canceled" }
        : unconfirmed
          ? { kind: "unconfirmed", isChecking }
          : { kind: "open" };

  return {
    status: "ready",
    partner: {
      id: partner.id,
      name: partner.username,
      portrait: null,
    },
    own: { status: ownStatus, lines: ownLines },
    other: { status: otherStatus, lines: otherLines },
    holdings,
    holdingsState:
      inventory.isPending || labels.isPending
        ? "loading"
        : inventory.isExhausted && holdings.length === 0
          ? "failed"
          : "ready",
    stage,
    pending,
    notice,
    dismissNotice: () => setNotice(null),
    setQuantity,
    add,
    ready: () => void setReadyFlag(true, "ready"),
    editItems: () => void setReadyFlag(false, "edit"),
    trade: () => void trade(),
    cancel: () => void cancel(),
    checkStatus: () => void checkStatus(),
  };
}
