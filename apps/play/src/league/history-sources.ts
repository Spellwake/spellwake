import type { SourceGap } from "../decks/deck-sources";

/** One finished match, as the player who played it sees it. */
export type HistoryMatch = {
  matchId: string;
  outcome: "won" | "lost" | "draw" | "no-result";
  /** Everyone else at the table, in seat order. */
  opponents: readonly string[];
  formatName: string;
  ranked: boolean;
  /** Games won by each side in a match of several; null for a single game. */
  games: { you: number; them: number } | null;
  /** When the match started, as an instant. */
  startedAt: string;
};

/** A match in the list; its facts are null while they are still being read. */
export type HistoryEntry = { matchId: string; match: HistoryMatch | null };

export type HistorySource =
  | { status: SourceGap }
  | {
      status: "ready";
      /** Newest first. */
      entries: readonly HistoryEntry[];
      hasMore: boolean;
      /** Shows the next older matches. */
      loadMore: () => void;
      /** Set when the list could not be read; it leaves by itself. */
      notice: string | null;
      dismissNotice: () => void;
    };

export const OUTCOME_LABEL: Record<HistoryMatch["outcome"], string> = {
  won: "Won",
  lost: "Lost",
  draw: "Draw",
  "no-result": "No result",
};

/** "Alex", "Alex and Morgan", "Alex and 4 others". */
export function opponentsLine(opponents: readonly string[]): string {
  const [first, second] = opponents;
  if (!first) return "No opponent";
  if (opponents.length === 1) return first;
  if (opponents.length === 2) return `${first} and ${second}`;
  return `${first} and ${opponents.length - 1} others`;
}

/** "Today", "Yesterday", or the day and month; the year once it is not this one. */
export function playedLine(startedAt: string, now: Date): string {
  const played = new Date(startedAt);
  const day = (date: Date) =>
    new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const days = Math.round((day(now) - day(played)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return played.toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    ...(played.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
  });
}
