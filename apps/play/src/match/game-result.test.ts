import { describe, expect, test } from "bun:test";
import type { PlayerSnapshot } from "@spellwake/application-contracts";
import { gameResult, gameRow, looksFinished } from "./game-result";
import type { GameResult } from "./match-sources";
import { previewSnapshot } from "./preview-table";

const GAME = "11111111-1111-4111-8111-111111111111";
const OTHER_GAME = "22222222-2222-4222-8222-222222222222";
const CONTEXT = "33333333-3333-4333-8333-333333333333";
const RESULT = "44444444-4444-4444-8444-444444444444";
const ME = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const THEM = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const THIRD = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

/** A table of the given seats, seen from the first. */
function table(
  seats: readonly { seatId: string; life?: string; eliminated?: boolean }[],
): PlayerSnapshot {
  const base = previewSnapshot();
  return {
    ...base,
    viewerSeatId: seats[0]!.seatId,
    players: seats.map((seat, index) => ({
      ...base.players[index]!,
      seatId: seat.seatId,
      life: seat.life ?? "20",
      eliminated: seat.eliminated ?? false,
    })),
  };
}

function row(
  state: string,
  result: Record<string, unknown> | null,
  id = GAME,
): Record<string, unknown> {
  return {
    id,
    contextId: id === GAME ? CONTEXT : null,
    gameIndex: "0",
    state,
    acceptedRevision: "7",
    result,
  };
}

const won = (winners: string[] | null, endReason: string) => ({
  id: RESULT,
  outcome: "win",
  winnerSeatIds: winners,
  endReason,
});

function resultOf(
  state: string,
  result: Record<string, unknown> | null,
  snapshot: PlayerSnapshot,
) {
  const found = gameRow([row(state, result)], GAME);
  if (!found) throw new Error("the game's row did not read");
  return gameResult(found, snapshot);
}

describe("a game's row among its match's games", () => {
  test("is the row with the game's id", () => {
    const rows = [row("reserved", null, OTHER_GAME), row("active", null)];
    expect(gameRow(rows, GAME)?.state).toBe("active");
  });

  test("is not read from a row the contract does not allow", () => {
    expect(gameRow([{ id: GAME, state: "completed" }], GAME)).toBeNull();
    expect(gameRow([row("active", null, OTHER_GAME)], GAME)).toBeNull();
  });
});

describe("how a game came out for the viewer", () => {
  const two = table([{ seatId: ME }, { seatId: THEM }]);

  test("is not known while the game is being played", () => {
    expect(resultOf("active", null, two)).toBeNull();
    expect(resultOf("reserved", null, two)).toBeNull();
  });

  test("is a win for a winning seat, with the cause in plain words", () => {
    expect(resultOf("completed", won([ME], "native_concession"), two)).toEqual({
      outcome: "won",
      reason: "Your opponent conceded.",
      winnerSeatIds: [ME],
    });
    expect(
      resultOf("completed", won([ME], "native_checked_losses"), two)?.reason,
    ).toBe("Your opponent is out.");
    expect(
      resultOf("completed", won([ME], "native_won_by_effect"), two)?.reason,
    ).toBe("A card won you the game.");
  });

  test("names every other player at a larger table", () => {
    const three = table([
      { seatId: ME },
      { seatId: THEM, eliminated: true },
      { seatId: THIRD, eliminated: true },
    ]);
    expect(
      resultOf("completed", won([ME], "native_concession"), three)?.reason,
    ).toBe("Every other player is out.");
  });

  test("is a loss for any other seat, explained only where the table shows why", () => {
    const dead = table([{ seatId: ME, life: "0" }, { seatId: THEM }]);
    expect(
      resultOf("completed", won([THEM], "native_checked_losses"), dead),
    ).toEqual({
      outcome: "lost",
      reason: "Your life reached 0.",
      winnerSeatIds: [THEM],
    });
    // A loss at 20 life could be an empty library or poison: nothing is claimed.
    expect(
      resultOf("completed", won([THEM], "native_checked_losses"), two)?.reason,
    ).toBeNull();
    expect(
      resultOf("completed", won([THEM], "native_concession"), two)?.reason,
    ).toBeNull();
    expect(
      resultOf("completed", won([THEM], "native_won_by_effect"), two)?.reason,
    ).toBe("A card won the game.");
  });

  test("falls back to who is left where the record names no winner", () => {
    const left = table([{ seatId: ME, eliminated: true }, { seatId: THEM }]);
    expect(
      resultOf("completed", won(null, "native_concession"), left),
    ).toMatchObject({ outcome: "lost", winnerSeatIds: [THEM] });
  });

  test("is a draw without a winner", () => {
    expect(
      resultOf(
        "completed",
        {
          id: RESULT,
          outcome: "draw",
          winnerSeatIds: [],
          endReason: "native_checked_losses",
        },
        two,
      ),
    ).toEqual({ outcome: "draw", reason: null, winnerSeatIds: [] });
  });

  test("is no result for a game that was not finished", () => {
    const none: GameResult = {
      outcome: "no-result",
      reason: "The game could not be finished.",
      winnerSeatIds: [],
    };
    expect(resultOf("no_contest", null, two)).toEqual(none);
    expect(resultOf("canceled", null, two)).toEqual(none);
    expect(
      resultOf(
        "completed",
        {
          id: RESULT,
          outcome: "no_contest",
          winnerSeatIds: [],
          endReason: "incident",
        },
        two,
      ),
    ).toEqual(none);
  });
});

describe("a table that looks finished", () => {
  test("has at most one seat still in", () => {
    expect(looksFinished(table([{ seatId: ME }, { seatId: THEM }]))).toBe(
      false,
    );
    expect(
      looksFinished(
        table([{ seatId: ME }, { seatId: THEM, eliminated: true }]),
      ),
    ).toBe(true);
  });
});
