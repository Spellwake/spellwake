import type { z } from "zod";
import {
  gameSummarySchema,
  type PlayerSnapshot,
} from "@spellwake/application-contracts";
import type { GameResult } from "./match-sources";

/** A game as its match records it. */
export type GameRow = z.infer<typeof gameSummarySchema>;

/** The one game's row among a match's games, where it reads as the contract says. */
export function gameRow(
  items: readonly unknown[],
  gameId: string,
): GameRow | null {
  for (const item of items) {
    const row = gameSummarySchema.safeParse(item);
    if (row.success && row.data.id === gameId) return row.data;
  }
  return null;
}

/**
 * Why the game ended, in the table's words. The game records a cause, not a
 * sentence, and only says what it knows: a loss is explained only where the
 * table itself shows why.
 */
function reasonFor(
  outcome: GameResult["outcome"],
  cause: string | null,
  snapshot: PlayerSnapshot,
): string | null {
  if (outcome === "no-result") return "The game could not be finished.";
  if (outcome === "draw") return null;
  const twoSeats = snapshot.players.length === 2;
  if (outcome === "won") {
    if (cause === "native_won_by_effect") return "A card won you the game.";
    if (!twoSeats) return "Every other player is out.";
    return cause === "native_concession"
      ? "Your opponent conceded."
      : "Your opponent is out.";
  }
  if (cause === "native_won_by_effect") return "A card won the game.";
  const viewer = snapshot.players.find(
    (player) => player.seatId === snapshot.viewerSeatId,
  );
  return cause === "native_checked_losses" && viewer && Number(viewer.life) <= 0
    ? "Your life reached 0."
    : null;
}

/**
 * How a game came out for the viewer, from its row in the match's games.
 * Null while the game is still being played.
 */
export function gameResult(
  row: GameRow,
  snapshot: PlayerSnapshot,
): GameResult | null {
  if (row.state === "reserved" || row.state === "active") return null;
  const recorded = row.result;
  if (
    row.state !== "completed" ||
    !recorded ||
    recorded.outcome === "no_contest"
  )
    return {
      outcome: "no-result",
      reason: reasonFor("no-result", null, snapshot),
      winnerSeatIds: [],
    };
  if (recorded.outcome === "draw")
    return { outcome: "draw", reason: null, winnerSeatIds: [] };
  // An older record may not name its winners; the table still shows who is left.
  const winnerSeatIds =
    recorded.winnerSeatIds ??
    snapshot.players
      .filter((player) => !player.eliminated)
      .map((player) => player.seatId);
  const outcome = winnerSeatIds.includes(snapshot.viewerSeatId)
    ? "won"
    : "lost";
  return {
    outcome,
    reason: reasonFor(outcome, recorded.endReason, snapshot),
    winnerSeatIds,
  };
}

/**
 * Whether the table itself looks finished: at most one seat is still in. The
 * result is asked for sooner then; it is never assumed from this.
 */
export function looksFinished(snapshot: PlayerSnapshot): boolean {
  return snapshot.players.filter((player) => !player.eliminated).length <= 1;
}
