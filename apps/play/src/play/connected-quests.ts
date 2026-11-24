import { useCallback, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  dailyWinPageSchema,
  questPageSchema,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { useInventoryGap } from "../collection/connected-inventory";
import { ownerBearer } from "../data/account";
import type { QuestsSource } from "./quest-sources";
import { shownQuests, swapFailure, swappableQuest, winsToday } from "./quests";

/** The most pages of daily wins read to find today's: a day's wins are one row. */
const MOST_WIN_PAGES = 8;

/**
 * The account's daily quests and today's wins. Quests are read each time the
 * decks are shown, which is where a player lands after a game. A row that
 * cannot be read is simply absent: the decks never wait on it.
 */
export function useConnectedQuests(): QuestsSource {
  const gap = useInventoryGap();
  const { state, services } = useAccount();
  const session = state.status === "ready" ? state.session : null;
  const owner = useMemo(
    () =>
      session?.accountId ? { ...session, accountId: session.accountId } : null,
    [session],
  );
  const [swapping, setSwapping] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const quests = useQuery({
    queryKey: ["daily-quests", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to see quests.");
      const read = await services.api.quests(
        await ownerBearer(services, owner),
        null,
        50,
        signal,
      );
      if (read.status !== "success") throw new Error("Quests unavailable.");
      return questPageSchema.parse(read.data);
    },
  });
  const utcDay = quests.data?.utcDay ?? null;
  const wins = useQuery({
    queryKey: ["daily-wins", owner?.accountId, owner?.sessionId, utcDay],
    enabled: !!owner && !!services && utcDay !== null,
    queryFn: async ({ signal }) => {
      if (!owner || !services || !utcDay) return null;
      const bearer = await ownerBearer(services, owner);
      let after: string | null = null;
      let today = { count: 0, coins: 0 };
      for (let page = 0; page < MOST_WIN_PAGES; page += 1) {
        const read = await services.api.dailyWins(bearer, after, 50, signal);
        if (read.status !== "success") return null;
        const parsed = dailyWinPageSchema.parse(read.data);
        const found = winsToday(parsed, utcDay);
        if (found.count > 0) today = found;
        after = parsed.nextCursor;
        if (!after) break;
      }
      return today;
    },
  });

  const swappable = quests.data ? swappableQuest(quests.data) : null;
  const refetch = quests.refetch;
  const swap = useCallback(() => {
    if (!owner || !services || !swappable || swapping) return;
    setSwapping(true);
    setNotice(null);
    void (async () => {
      try {
        const sent = await services.api.rerollQuest(
          await ownerBearer(services, owner),
          { requestId: crypto.randomUUID(), assignmentId: swappable },
        );
        if (sent.status === "fail") setNotice(swapFailure(sent.data));
        else if (sent.status !== "success")
          setNotice("Couldn’t swap the quest.");
      } catch {
        setNotice("Couldn’t swap the quest.");
      }
      await refetch();
      setSwapping(false);
    })();
  }, [owner, services, swappable, swapping, refetch]);

  const shown = useMemo(
    () => (quests.data ? shownQuests(quests.data) : []),
    [quests.data],
  );
  if (gap) return { status: gap };
  if (quests.isPending) return { status: "loading" };
  if (!quests.data) return { status: "unavailable" };
  return {
    status: "ready",
    quests: shown,
    swappable,
    swapping,
    swap,
    notice,
    wins: wins.data ?? null,
  };
}
