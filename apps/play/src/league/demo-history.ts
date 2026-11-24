import { useMemo, useState } from "react";
import type { HistoryMatch, HistorySource } from "./history-sources";

const HOUR = 3_600_000;
type Line = [
  HistoryMatch["outcome"],
  string[],
  string,
  boolean,
  [number, number] | null,
  number,
];
// Hours ago, newest first.
const LINES: readonly Line[] = [
  ["won", ["Alex"], "Modern", true, [2, 1], 2],
  ["lost", ["Morgan"], "Modern", true, [0, 2], 5],
  ["won", ["Sam", "Riley", "Jordan"], "Commander", false, null, 26],
  ["won", ["Thalia"], "Pioneer", true, [2, 0], 31],
  ["draw", ["Casey"], "Standard", false, null, 74],
  [
    "lost",
    ["Alex", "Morgan", "Sam", "Riley", "Jordan"],
    "Commander",
    false,
    null,
    98,
  ],
  [
    "won",
    ["A player with a very long name indeed"],
    "Legacy",
    true,
    [2, 1],
    190,
  ],
  ["no-result", ["Devon"], "Modern", true, null, 260],
  ["lost", ["Thalia"], "Pauper", false, [1, 2], 500],
  ["won", ["Morgan"], "Modern", true, [2, 0], 9500],
];

const PAGE = 8;

/**
 * The preview's match history: ten finished matches, newest first, shown
 * eight at a time. `slow` leaves the newest two still being read.
 */
export function useDemoHistory(
  state: "list" | "empty" | "slow" | "failed" = "list",
): HistorySource {
  const [shown, setShown] = useState(PAGE);
  const [dismissed, setDismissed] = useState(false);
  return useMemo(() => {
    const now = Date.now();
    const all =
      state === "empty" || state === "failed"
        ? []
        : LINES.map(
            ([outcome, opponents, formatName, ranked, games, ago], index) => ({
              matchId: `preview-match-${index}`,
              match:
                state === "slow" && index < 2
                  ? null
                  : {
                      matchId: `preview-match-${index}`,
                      outcome,
                      opponents,
                      formatName,
                      ranked,
                      games: games && { you: games[0], them: games[1] },
                      startedAt: new Date(now - ago * HOUR).toISOString(),
                    },
            }),
          );
    return {
      status: "ready",
      entries: all.slice(0, shown),
      hasMore: shown < all.length,
      loadMore: () => setShown((count) => count + PAGE),
      notice:
        state === "failed" && !dismissed ? "Couldn’t load your matches." : null,
      dismissNotice: () => setDismissed(true),
    };
  }, [state, shown, dismissed]);
}
