import { describe, expect, test } from "bun:test";
import { matchDetailSchema } from "@spellwake/application-contracts";
import {
  gameRows,
  ownDecisions,
  scoreLine,
  seriesStanding,
  timeLeft,
} from "./match-series";

const id = (n: number) =>
  `${String(n).padStart(8, "0")}-0000-4000-8000-000000000000`;
const MATCH = id(1);
const ME = id(11);
const THEM = id(12);
const [G1, G2, G3] = [id(21), id(22), id(23)];
const AT = "2026-11-24T10:00:00Z";

function detail(
  state: string,
  structure: "bo1" | "bo3" = "bo3",
  winners: string[] | null = null,
) {
  return matchDetailSchema.parse({
    accountId: id(2),
    match: {
      id: MATCH,
      formatId: id(3),
      profileId: id(4),
      mode: "open",
      competitive: false,
      structure,
      state,
      revision: "9",
      startedAt: AT,
      currentGameId: null,
      reservedGameId: null,
      availability: "available",
      availabilityRevision: "1",
    },
    ownSeatId: ME,
    result: winners && {
      id: id(5),
      outcome: "win",
      committedAt: AT,
      rankingFormatId: null,
      rankSequence: null,
      winnerSeatIds: winners,
      terminalKind: "game_outcome",
    },
  });
}

function game(
  gameId: string,
  index: number,
  state: string,
  winner: string | null = null,
) {
  return {
    id: gameId,
    contextId: state === "reserved" ? null : id(100 + index),
    gameIndex: String(index),
    state,
    acceptedRevision: "3",
    result: winner && {
      id: id(200 + index),
      outcome: "win",
      winnerSeatIds: [winner],
      endReason: "native_checked_losses",
    },
  };
}

function decision(after: string, state: string, seatId = ME) {
  return {
    id: id(300),
    seatId,
    decisionKind: "deck",
    basisRevision: "8",
    deadlineAt: null,
    clockRevision: "0",
    state,
    afterGameId: after,
    selectedDeckRevisionId: null,
    openedAt: AT,
    deadlinePolicyRef: null,
    deadlineRevision: "0",
    deadlineAvailabilityRevision: null,
    deadlineRemainingUs: null,
    deadlineAccountedAt: null,
  };
}

describe("a match's games and decisions as read", () => {
  test("come back in the order played, without rows the contract refuses", () => {
    const rows = gameRows([
      game(G2, 1, "active"),
      { id: G3 },
      game(G1, 0, "completed", ME),
    ]);
    expect(rows.map((row) => row.id)).toEqual([G1, G2]);
    expect(ownDecisions([decision(G1, "pending"), { id: "x" }])).toHaveLength(
      1,
    );
  });
});

describe("where a match stands after one of its games", () => {
  const one = gameRows([game(G1, 0, "completed", ME)]);
  const asked = ownDecisions([decision(G1, "pending")]);

  test("is nothing for a single game or a game still being played", () => {
    expect(
      seriesStanding(detail("completed", "bo1", [ME]), one, [], G1, 2),
    ).toBeNull();
    expect(
      seriesStanding(
        detail("active"),
        gameRows([game(G1, 0, "active")]),
        [],
        G1,
        2,
      ),
    ).toBeNull();
  });

  test("waits on the viewer while their decision is open", () => {
    expect(seriesStanding(detail("between_games"), one, asked, G1, 2)).toEqual({
      gameNumber: 1,
      score: { you: 1, them: 0 },
      next: {
        kind: "deciding",
        decisionId: id(300),
        deadlineAt: null,
        matchRevision: "9",
      },
    });
  });

  test("waits on the others once the viewer has answered", () => {
    expect(
      seriesStanding(
        detail("between_games"),
        one,
        ownDecisions([decision(G1, "answered")]),
        G1,
        2,
      )?.next,
    ).toEqual({ kind: "waiting" });
    // Another seat's open decision is not the viewer's to answer.
    expect(
      seriesStanding(
        detail("between_games"),
        one,
        ownDecisions([decision(G1, "pending", THEM)]),
        G1,
        2,
      )?.next,
    ).toEqual({ kind: "waiting" });
    // A game that is reserved but not dealt is still being waited for.
    expect(
      seriesStanding(
        detail("between_games"),
        gameRows([game(G1, 0, "completed", ME), game(G2, 1, "reserved")]),
        asked,
        G1,
        2,
      )?.next,
    ).toEqual({ kind: "waiting" });
  });

  test("names the next game once it can be played", () => {
    expect(
      seriesStanding(
        detail("active"),
        gameRows([game(G1, 0, "completed", ME), game(G2, 1, "active")]),
        [],
        G1,
        2,
      )?.next,
    ).toEqual({ kind: "started", gameId: G2, contextId: id(101) });
  });

  test("counts games won up to the finished game", () => {
    const games = gameRows([
      game(G1, 0, "completed", ME),
      game(G2, 1, "completed", THEM),
      game(G3, 2, "active"),
    ]);
    expect(seriesStanding(detail("active"), games, [], G2, 2)).toMatchObject({
      gameNumber: 2,
      score: { you: 1, them: 1 },
    });
  });

  test("gives no pair of numbers at a larger table", () => {
    expect(
      seriesStanding(detail("between_games"), one, asked, G1, 4)?.score,
    ).toBeNull();
  });

  test("is over with the match's own outcome", () => {
    const games = gameRows([
      game(G1, 0, "completed", THEM),
      game(G2, 1, "completed", THEM),
    ]);
    expect(
      seriesStanding(detail("completed", "bo3", [THEM]), games, [], G2, 2),
    ).toEqual({
      gameNumber: 2,
      score: { you: 0, them: 2 },
      next: { kind: "over", outcome: "lost" },
    });
    expect(
      seriesStanding(detail("completed", "bo3", [ME]), games, [], G2, 2)?.next,
    ).toEqual({ kind: "over", outcome: "won" });
    expect(
      seriesStanding(detail("no_contest"), games, [], G2, 2)?.next,
    ).toEqual({ kind: "over", outcome: "no-result" });
  });
});

describe("the words for a score and a time", () => {
  test("say who is ahead", () => {
    expect(scoreLine({ you: 1, them: 0 })).toBe("You lead 1–0");
    expect(scoreLine({ you: 1, them: 1 })).toBe("Tied 1–1");
    expect(scoreLine({ you: 0, them: 1 })).toBe("You trail 0–1");
  });

  test("count down in minutes and seconds and stop at nothing", () => {
    const now = Date.parse(AT);
    expect(timeLeft("2026-11-24T10:02:41Z", now)).toBe("2:41");
    expect(timeLeft("2026-11-24T10:00:05Z", now)).toBe("0:05");
    expect(timeLeft("2026-11-24T09:59:00Z", now)).toBe("0:00");
  });
});
