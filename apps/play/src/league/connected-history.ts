import { useMemo, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import {
  matchDetailSchema,
  matchListSchema,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import {
  READ_ATTEMPTS,
  useInventoryGap,
} from "../collection/connected-inventory";
import { ownerBearer } from "../data/account";
import { useDeckFormats } from "../data/deck-commands";
import { gameRows } from "../match/match-series";
import {
  finishedNewestFirst,
  historyMatch,
  seatRows,
  type MatchSummary,
} from "./history-read";
import type { HistorySource } from "./history-sources";

/** Matches shown at a time; each needs its own reads. */
const SHOWN = 12;
/** The most pages of matches read for one history: 500 matches. */
const MOST_PAGES = 10;
const FAILED = "Couldn’t load your matches.";

/**
 * The account's finished matches. `/matches` lists every match the account
 * played in the order of their ids and says nothing of how each came out, so
 * the whole list is read and sorted here, and each match shown reads its own
 * result, seats and, for a match of several games, its games.
 */
export function useConnectedHistory(): HistorySource {
  const gap = useInventoryGap();
  const { state, services } = useAccount();
  const session = state.status === "ready" ? state.session : null;
  const owner = useMemo(
    () =>
      session?.accountId ? { ...session, accountId: session.accountId } : null,
    [session],
  );
  const formatsRead = useDeckFormats();
  const formatNames = useMemo(
    () =>
      new Map(
        (formatsRead.data?.pages ?? []).flatMap((entry) =>
          entry.items.map((item) => [item.id, item.name] as const),
        ),
      ),
    [formatsRead.data],
  );
  const [shown, setShown] = useState(SHOWN);
  const [dismissed, setDismissed] = useState(false);

  const list = useQuery({
    queryKey: ["history-matches", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }): Promise<MatchSummary[]> => {
      if (!owner || !services) throw new Error(FAILED);
      const bearer = await ownerBearer(services, owner);
      const all: MatchSummary[] = [];
      let after: string | null = null;
      for (let page = 0; page < MOST_PAGES; page += 1) {
        const read = await services.api.matches(bearer, after, 50, signal);
        if (read.status !== "success") throw new Error(FAILED);
        const parsed = matchListSchema.parse(read.data);
        all.push(...parsed.items);
        after = parsed.nextCursor;
        if (!after) break;
      }
      return finishedNewestFirst(all);
    },
  });
  const listed = useMemo(
    () => (list.data ?? []).slice(0, shown),
    [list.data, shown],
  );

  const rows = useQueries({
    queries: listed.map((match) => ({
      queryKey: ["history-match", owner?.accountId, match.id],
      enabled: !!owner && !!services,
      // A finished match does not change.
      staleTime: Infinity,
      retry: READ_ATTEMPTS - 1,
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        if (!owner || !services) throw new Error(FAILED);
        const bearer = await ownerBearer(services, owner);
        const [detailRead, seatsRead, gamesRead] = await Promise.all([
          services.api.matchDetail(bearer, match.id, signal),
          services.api.matchSeats(bearer, match.id, null, 12, signal),
          match.structure === "bo3"
            ? services.api.matchGames(bearer, match.id, null, 10, signal)
            : null,
        ]);
        if (detailRead.status !== "success" || seatsRead.status !== "success")
          throw new Error(FAILED);
        const items = (data: Record<string, unknown>): unknown[] =>
          Array.isArray(data["items"]) ? data["items"] : [];
        return {
          detail: matchDetailSchema.parse(detailRead.data),
          seats: seatRows(items(seatsRead.data)),
          games:
            gamesRead?.status === "success"
              ? gameRows(items(gamesRead.data))
              : [],
        };
      },
    })),
  });

  if (gap) return { status: gap };
  if (list.isPending) return { status: "loading" };
  return {
    status: "ready",
    // A match whose own reads failed is left out rather than shown half known.
    entries: listed.flatMap((match, index) => {
      const row = rows[index];
      if (row?.isError) return [];
      return [
        {
          matchId: match.id,
          match: row?.data
            ? historyMatch(
                row.data.detail,
                row.data.seats,
                row.data.games,
                formatNames.get(match.formatId) ?? "Match",
              )
            : null,
        },
      ];
    }),
    hasMore: shown < (list.data?.length ?? 0),
    loadMore: () => setShown((count) => count + SHOWN),
    notice: list.isError && !dismissed ? FAILED : null,
    dismissNotice: () => setDismissed(true),
  };
}
