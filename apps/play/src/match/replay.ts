import {
  playerHistoryQuerySchema,
  playerReplayRequestSchema,
  type PlayerCursor,
  type PlayerHistoryQuery,
  type PlayerReplayRequest,
  type PlayerVisibleEvent,
} from "@spellwake/application-contracts";

/** A page of a game's history, read from its start. Null where the contract would refuse it. */
export function historyQuery(input: {
  gameId: string;
  contextId: string;
  cursor: PlayerCursor | null;
}): PlayerHistoryQuery | null {
  const query = playerHistoryQuerySchema.safeParse({ ...input, limit: "100" });
  return query.success ? query.data : null;
}

/**
 * The request for the table as it stood after one event, seen from the
 * player's own seat, or with every hand shown once the game is over. Null
 * where the contract would refuse it, so nothing invalid is sent.
 */
export function replayRequest(input: {
  gameId: string;
  contextId: string;
  atEventSequence: string;
  perspectiveSeatId: string;
  everyHand: boolean;
}): PlayerReplayRequest | null {
  const request = playerReplayRequestSchema.safeParse({
    gameId: input.gameId,
    contextId: input.contextId,
    atEventSequence: input.atEventSequence,
    perspectiveSeatId: input.perspectiveSeatId,
    mode: input.everyHand ? "completed-full-information" : "historical-seat",
    history: {
      gameId: input.gameId,
      contextId: input.contextId,
      cursor: null,
      limit: "1",
    },
  });
  return request.success ? request.data : null;
}

/** A game's events as steps to move through, with where each turn begins. */
export type Timeline = {
  /** Each step's event sequence, in order. */
  steps: readonly string[];
  /** The step each turn begins at, in order. */
  turnStarts: readonly number[];
};

/** The steps of a game from its events, oldest first; repeated events count once. */
export function timeline(events: readonly PlayerVisibleEvent[]): Timeline {
  const ordered = [
    ...new Map(events.map((event) => [event.eventSequence, event])).values(),
  ].sort((a, b) =>
    BigInt(a.eventSequence) < BigInt(b.eventSequence) ? -1 : 1,
  );
  const turnStarts: number[] = [];
  let turn: string | null = null;
  ordered.forEach((event, index) => {
    for (const change of event.changes) {
      if (change.kind !== "turn" || !change.turn) continue;
      if (change.turn.number !== turn) {
        turn = change.turn.number;
        turnStarts.push(index);
      }
    }
  });
  return { steps: ordered.map((event) => event.eventSequence), turnStarts };
}

/** The first step of the turn after the given step's turn; the last step where there is none. */
export function nextTurn(line: Timeline, at: number): number {
  return (
    line.turnStarts.find((start) => start > at) ??
    Math.max(0, line.steps.length - 1)
  );
}

/**
 * The first step of the given step's turn, or of the turn before it when the
 * step already is its turn's first; the opening step where there is none.
 */
export function previousTurn(line: Timeline, at: number): number {
  const earlier = line.turnStarts.filter((start) => start < at);
  return earlier.at(-1) ?? 0;
}
