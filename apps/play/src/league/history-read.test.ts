import { describe, expect, test } from "bun:test";
import {
  matchDetailSchema,
  matchSummarySchema,
} from "@spellwake/application-contracts";
import { gameRows } from "../match/match-series";
import { finishedNewestFirst, historyMatch, seatRows } from "./history-read";
import { opponentsLine, playedLine } from "./history-sources";

const id = (n: number) =>
  `${String(n).padStart(8, "0")}-0000-4000-8000-000000000000`;
const ME = id(11);
const THEM = id(12);

function summary(
  n: number,
  state: string,
  startedAt: string,
  structure: "bo1" | "bo3" = "bo1",
  competitive = false,
) {
  return matchSummarySchema.parse({
    id: id(n),
    formatId: id(3),
    profileId: id(4),
    mode: competitive ? "collection" : "open",
    competitive,
    structure,
    state,
    revision: "9",
    startedAt,
    currentGameId: null,
    reservedGameId: null,
    availability: "available",
    availabilityRevision: "1",
  });
}

function detail(
  match: ReturnType<typeof summary>,
  outcome: "win" | "draw" | "no_contest" | null,
  winners: string[] | null = [],
) {
  return matchDetailSchema.parse({
    accountId: id(2),
    match,
    ownSeatId: ME,
    result: outcome && {
      id: id(5),
      outcome,
      committedAt: "2026-11-24T12:00:00Z",
      rankingFormatId: null,
      rankSequence: null,
      winnerSeatIds: winners,
      terminalKind: "game_outcome",
    },
  });
}

const seat = (seatId: string, index: number, username: string | null) => ({
  id: seatId,
  seatIndex: String(index),
  controllerKind: username ? "human" : "ai",
  person: username && { id: id(50 + index), username, isDeleted: false },
  clock: null,
});

const game = (index: number, winner: string) => ({
  id: id(20 + index),
  contextId: id(100 + index),
  gameIndex: String(index),
  state: "completed",
  acceptedRevision: "3",
  result: {
    id: id(200 + index),
    outcome: "win",
    winnerSeatIds: [winner],
    endReason: "native_checked_losses",
  },
});

describe("the matches a history lists", () => {
  test("are the finished ones, newest first", () => {
    const listed = finishedNewestFirst([
      summary(1, "completed", "2026-11-20T10:00:00Z"),
      summary(2, "active", "2026-11-24T10:00:00Z"),
      summary(3, "no_contest", "2026-11-23T10:00:00Z"),
      summary(4, "completed", "2026-11-21T10:00:00Z"),
    ]);
    expect(listed.map((match) => match.id)).toEqual([id(3), id(4), id(1)]);
  });
});

describe("a finished match as its row shows it", () => {
  const seats = seatRows([seat(THEM, 1, "alex"), seat(ME, 0, "me")]);
  const single = summary(1, "completed", "2026-11-20T10:00:00Z");
  const series = summary(1, "completed", "2026-11-20T10:00:00Z", "bo3", true);

  test("is won or lost by who the match names as its winner", () => {
    expect(
      historyMatch(detail(single, "win", [ME]), seats, [], "Modern"),
    ).toEqual({
      matchId: id(1),
      outcome: "won",
      opponents: ["alex"],
      formatName: "Modern",
      ranked: false,
      games: null,
      startedAt: "2026-11-20T10:00:00Z",
    });
    expect(
      historyMatch(detail(single, "win", [THEM]), seats, [], "Modern").outcome,
    ).toBe("lost");
  });

  test("counts the games of a match of several between two players", () => {
    const games = gameRows([game(0, ME), game(1, THEM), game(2, ME)]);
    expect(
      historyMatch(detail(series, "win", [ME]), seats, games, "Modern"),
    ).toMatchObject({
      outcome: "won",
      ranked: true,
      games: { you: 2, them: 1 },
    });
  });

  test("reads an unnamed winner from the games, and claims nothing without them", () => {
    const games = gameRows([game(0, THEM), game(1, THEM)]);
    expect(
      historyMatch(detail(series, "win", null), seats, games, "Modern").outcome,
    ).toBe("lost");
    expect(
      historyMatch(detail(single, "win", null), seats, [], "Modern").outcome,
    ).toBe("no-result");
  });

  test("is a draw or no result as the match records it", () => {
    expect(
      historyMatch(detail(single, "draw"), seats, [], "Modern").outcome,
    ).toBe("draw");
    expect(
      historyMatch(detail(single, "no_contest"), seats, [], "Modern").outcome,
    ).toBe("no-result");
    expect(
      historyMatch(detail(single, null), seats, [], "Modern").outcome,
    ).toBe("no-result");
  });

  test("names every other seat in seat order, and the AI bot by its own name", () => {
    const table = seatRows([
      seat(id(14), 3, null),
      seat(ME, 0, "me"),
      seat(id(13), 2, "morgan"),
      seat(THEM, 1, "alex"),
    ]);
    const row = historyMatch(
      detail(series, "win", [ME]),
      table,
      [],
      "Commander",
    );
    expect(row.opponents).toEqual(["alex", "morgan", "Spellwake Bot"]);
    // No pair of numbers says a table of four.
    expect(row.games).toBeNull();
  });
});

describe("the words of a history row", () => {
  test("name one, two or many opponents", () => {
    expect(opponentsLine([])).toBe("No opponent");
    expect(opponentsLine(["Alex"])).toBe("Alex");
    expect(opponentsLine(["Alex", "Morgan"])).toBe("Alex and Morgan");
    expect(opponentsLine(["Alex", "Morgan", "Sam"])).toBe("Alex and 2 others");
  });

  test("say today and yesterday by the calendar day", () => {
    const now = new Date(2026, 10, 24, 9, 0);
    expect(playedLine(new Date(2026, 10, 24, 0, 5).toISOString(), now)).toBe(
      "Today",
    );
    expect(playedLine(new Date(2026, 10, 23, 23, 55).toISOString(), now)).toBe(
      "Yesterday",
    );
    expect(
      playedLine(new Date(2026, 10, 22, 12, 0).toISOString(), now),
    ).not.toBe("Yesterday");
  });
});
