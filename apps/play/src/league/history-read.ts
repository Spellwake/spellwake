import type { z } from "zod";
import {
  matchSeatSchema,
  matchSummarySchema,
  type MatchDetail,
} from "@spellwake/application-contracts";
import type { GameRow } from "../match/game-result";
import type { HistoryMatch } from "./history-sources";

export type MatchSummary = z.infer<typeof matchSummarySchema>;
type MatchSeat = z.infer<typeof matchSeatSchema>;

/** The seats of a match that read as the contract says, in seat order. */
export function seatRows(items: readonly unknown[]): MatchSeat[] {
  return items
    .flatMap((item) => {
      const seat = matchSeatSchema.safeParse(item);
      return seat.success ? [seat.data] : [];
    })
    .sort((a, b) => Number(a.seatIndex) - Number(b.seatIndex));
}

/**
 * The finished matches of those read, newest first. The server lists matches
 * in the order of their ids, which is not the order they were played.
 */
export function finishedNewestFirst(
  matches: readonly MatchSummary[],
): MatchSummary[] {
  return matches
    .filter(
      (match) => match.state === "completed" || match.state === "no_contest",
    )
    .sort((a, b) => Date.parse(b.startedAt) - Date.parse(a.startedAt));
}

/** Games won by the viewer and by the other seat, where each game names its winner. */
function gamesWon(
  games: readonly GameRow[],
  ownSeatId: string,
): { you: number; them: number } {
  const won = (mine: boolean) =>
    games.filter(
      (game) =>
        game.result?.outcome === "win" &&
        game.result.winnerSeatIds !== null &&
        game.result.winnerSeatIds.includes(ownSeatId) === mine,
    ).length;
  return { you: won(true), them: won(false) };
}

/** One finished match as its row shows it. */
export function historyMatch(
  detail: MatchDetail,
  seats: readonly MatchSeat[],
  games: readonly GameRow[],
  formatName: string,
): HistoryMatch {
  const { match, ownSeatId, result } = detail;
  const others = seats.filter((seat) => seat.id !== ownSeatId);
  const tally = gamesWon(games, ownSeatId);
  let outcome: HistoryMatch["outcome"] = "no-result";
  if (result?.outcome === "draw") outcome = "draw";
  else if (result?.outcome === "win") {
    if (result.winnerSeatIds)
      outcome = result.winnerSeatIds.includes(ownSeatId) ? "won" : "lost";
    // An older record may not name its winner; its games still do.
    else if (tally.you !== tally.them)
      outcome = tally.you > tally.them ? "won" : "lost";
  }
  return {
    matchId: match.id,
    outcome,
    opponents: others.map((seat) => seat.person?.username ?? "Spellwake Bot"),
    formatName,
    ranked: match.competitive,
    games: match.structure === "bo3" && others.length === 1 ? tally : null,
    startedAt: match.startedAt,
  };
}
