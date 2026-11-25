import { useCallback, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { notificationPageSchema } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { useInventoryGap } from "../collection/connected-inventory";
import { ownerBearer } from "../data/account";
import { tradePath } from "../trade/trade-sources";
import type { UpdatesSource } from "./updates-source";
import { otherParty, targetOf, toMarkRead, updates } from "./updates";

/** The most pages of updates read: five hundred. */
const MOST_PAGES = 5;
/** How often the header learns of new updates. */
const POLL_MS = 60_000;

/**
 * The account's updates: trade offers, accepted trades and sold listings.
 * They are read once a minute so the header can show that something is new.
 * Opening the Updates thread marks what it shows as read; those stay marked
 * new until the thread is left, so the visit that read them can tell.
 */
export function useConnectedUpdates(): UpdatesSource {
  const gap = useInventoryGap();
  const navigate = useNavigate();
  const { state, services } = useAccount();
  const session = state.status === "ready" ? state.session : null;
  const owner = useMemo(
    () =>
      session?.accountId ? { ...session, accountId: session.accountId } : null,
    [session],
  );
  const read = useQuery({
    queryKey: ["updates", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    refetchInterval: POLL_MS,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to see updates.");
      const bearer = await ownerBearer(services, owner);
      const items: unknown[] = [];
      let after: string | null = null;
      let unread = 0;
      for (let page = 0; page < MOST_PAGES; page += 1) {
        const result = await services.api.notifications(
          bearer,
          after,
          100,
          signal,
        );
        if (result.status !== "success")
          throw new Error("Updates unavailable.");
        const parsed = notificationPageSchema.parse(result.data);
        if (page === 0) unread = Number(parsed.unreadCount);
        items.push(...parsed.items);
        after = parsed.nextCursor;
        if (!after) break;
      }
      return { items, unread };
    },
  });
  const [shownNew, setShownNew] = useState<ReadonlySet<string>>(new Set());
  const [notice, setNotice] = useState<string | null>(null);
  const marking = useRef(false);
  const list = useMemo(
    () =>
      updates(read.data?.items ?? []).map((update) => ({
        ...update,
        isNew: update.isNew || shownNew.has(update.id),
      })),
    [read.data, shownNew],
  );
  const refetch = read.refetch;
  const markSeen = useCallback(() => {
    if (!owner || !services || marking.current) return;
    const ids = toMarkRead(updates(read.data?.items ?? []));
    if (ids.length === 0) return;
    marking.current = true;
    setShownNew((current) => new Set([...current, ...ids]));
    void (async () => {
      try {
        await services.api.markNotificationsRead(
          await ownerBearer(services, owner),
          { requestId: crypto.randomUUID(), notificationIds: ids },
        );
      } finally {
        marking.current = false;
        void refetch();
      }
    })();
  }, [owner, services, read.data, refetch]);
  const open = useCallback(
    (update: (typeof list)[number]) => {
      const target = targetOf(update);
      if (!target || !owner || !services) return;
      setNotice(null);
      if (target.to === "listings") {
        void navigate("/market/listings?mine=true");
        return;
      }
      void (async () => {
        const trade = await services.api.trade(
          await ownerBearer(services, owner),
          owner.accountId,
          target.tradeId,
        );
        if (trade.status === "success")
          void navigate(
            tradePath(otherParty(trade.data.trade, owner.accountId)),
          );
        else setNotice("This trade is no longer available.");
      })();
    },
    [owner, services, navigate],
  );
  if (gap) return { status: gap };
  if (read.isPending) return { status: "loading" };
  if (!read.data) return { status: "unavailable" };
  return {
    status: "ready",
    items: list,
    // Seen this visit but not yet confirmed read by the server: no longer new.
    unread: Math.max(
      0,
      read.data.unread -
        updates(read.data.items).filter(
          (update) => update.isNew && shownNew.has(update.id),
        ).length,
    ),
    markSeen,
    open,
    notice,
  };
}
