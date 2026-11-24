import type { z } from "zod";
import {
  gameSummarySchema,
  ownDecisionSchema,
  type MatchDetail,
} from "@spellwake/application-contracts";
import type { GameRow } from "./game-result";
import type { GameResult } from "./match-sources";

type OwnDecision = z.infer<typeof ownDecisionSchema>;

/** Every game of a match that reads as the contract says, in the order they were played. */
export function gameRows(items: readonly unknown[]): GameRow[] {
  return items
    .flatMap((item) => {
      const row = gameSummarySchema.safeParse(item);
      return row.success ? [row.data] : [];
    })
    .sort((a, b) =>
      BigInt(a.gameIndex) < BigInt(b.gameIndex)
        ? -1
        : BigInt(a.gameIndex) > BigInt(b.gameIndex)
          ? 1
          : 0,
    );
}

/** The viewer's own between-games decisions that read as the contract says. */
export function ownDecisions(items: readonly unknown[]): OwnDecision[] {
  return items.flatMap((item) => {
    const row = ownDecisionSchema.safeParse(item);
    return row.success ? [row.data] : [];
  });
}

/** Where a match of several games stands for the viewer after one of its games. */
export type SeriesStanding = {
  gameNumber: number;
  score: { you: number; them: number } | null;
  next:
    | { kind: "over"; outcome: GameResult["outcome"] }
    | {
        kind: "deciding";
        decisionId: string;
        deadlineAt: string | null;
        /** The match revision the answer must name. */
        matchRevision: string;
      }
    | { kind: "waiting" }
    | {
        /** The next game is ready to be played. */
        kind: "started";
        gameId: string;
        contextId: string;
      };
};

function matchOutcome(detail: MatchDetail): GameResult["outcome"] {
  const recorded = detail.result;
  if (!recorded || recorded.outcome === "no_contest") return "no-result";
  if (recorded.outcome === "draw") return "draw";
  return recorded.winnerSeatIds?.includes(detail.ownSeatId) ? "won" : "lost";
}

/**
 * Reads where the match stands after the given game. Null for a single game,
 * and while the game is still being played or its place cannot be read.
 */
export function seriesStanding(
  detail: MatchDetail,
  games: readonly GameRow[],
  decisions: readonly OwnDecision[],
  gameId: string,
  seatCount: number,
): SeriesStanding | null {
  if (detail.match.structure === "bo1") return null;
  const place = games.findIndex((game) => game.id === gameId);
  const finished = games[place];
  if (!finished || finished.state === "reserved" || finished.state === "active")
    return null;
  const played = games.slice(0, place + 1);
  const won = (mine: boolean) =>
    played.filter(
      (game) =>
        game.result?.outcome === "win" &&
        game.result.winnerSeatIds !== null &&
        game.result.winnerSeatIds.includes(detail.ownSeatId) === mine,
    ).length;
  const standing = {
    gameNumber: place + 1,
    score: seatCount === 2 ? { you: won(true), them: won(false) } : null,
  };
  if (
    detail.result ||
    detail.match.state === "completed" ||
    detail.match.state === "no_contest"
  )
    return {
      ...standing,
      next: { kind: "over", outcome: matchOutcome(detail) },
    };
  const following = games[place + 1];
  if (following?.state === "active" && following.contextId)
    return {
      ...standing,
      next: {
        kind: "started",
        gameId: following.id,
        contextId: following.contextId,
      },
    };
  const asked = decisions.find(
    (decision) =>
      decision.afterGameId === gameId &&
      decision.seatId === detail.ownSeatId &&
      decision.state === "pending",
  );
  return {
    ...standing,
    next:
      asked && !following
        ? {
            kind: "deciding",
            decisionId: asked.id,
            deadlineAt: asked.deadlineAt,
            matchRevision: detail.match.revision,
          }
        : { kind: "waiting" },
  };
}

/** "You lead 1–0", "Tied 1–1", "You trail 0–1". */
export function scoreLine(score: { you: number; them: number }): string {
  const numbers = `${score.you}–${score.them}`;
  if (score.you === score.them) return `Tied ${numbers}`;
  return `${score.you > score.them ? "You lead" : "You trail"} ${numbers}`;
}

/** The time left as "2:41"; "0:00" once it has run out. */
export function timeLeft(deadlineAt: string, now: number): string {
  const seconds = Math.max(0, Math.ceil((Date.parse(deadlineAt) - now) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
