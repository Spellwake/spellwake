import { describe, expect, test } from "bun:test";
import type { PlayerVisibleEvent } from "@spellwake/application-contracts";
import {
  historyQuery,
  nextTurn,
  previousTurn,
  replayRequest,
  timeline,
} from "./replay";

const GAME = "11111111-1111-4111-8111-111111111111";
const CONTEXT = "22222222-2222-4222-8222-222222222222";
const SEAT = "33333333-3333-4333-8333-333333333333";

/** An event as the timeline reads it: its sequence and whether it starts a turn. */
function event(sequence: number, turn: number | null): PlayerVisibleEvent {
  return {
    eventSequence: String(sequence),
    changes:
      turn === null
        ? [{ kind: "life", seatId: SEAT, before: "20", after: "19" }]
        : [
            {
              kind: "turn",
              turn: {
                number: String(turn),
                activeSeatIds: [SEAT],
                prioritySeatIds: [SEAT],
                phase: "beginning",
                step: "untap",
              },
            },
          ],
  } as unknown as PlayerVisibleEvent;
}

describe("the requests a replay sends", () => {
  test("read history from the start, a hundred events a page", () => {
    expect(
      historyQuery({ gameId: GAME, contextId: CONTEXT, cursor: null }),
    ).toEqual({
      gameId: GAME,
      contextId: CONTEXT,
      cursor: null,
      limit: "100",
    });
  });

  test("ask for the table after one event, from the player's seat or with every hand", () => {
    const own = replayRequest({
      gameId: GAME,
      contextId: CONTEXT,
      atEventSequence: "12",
      perspectiveSeatId: SEAT,
      everyHand: false,
    });
    expect(own).toMatchObject({
      atEventSequence: "12",
      mode: "historical-seat",
    });
    expect(own?.history).toEqual({
      gameId: GAME,
      contextId: CONTEXT,
      cursor: null,
      limit: "1",
    });
    expect(
      replayRequest({
        gameId: GAME,
        contextId: CONTEXT,
        atEventSequence: "12",
        perspectiveSeatId: SEAT,
        everyHand: true,
      })?.mode,
    ).toBe("completed-full-information");
  });

  test("are not made where the contract would refuse them", () => {
    expect(
      replayRequest({
        gameId: "not-a-game",
        contextId: CONTEXT,
        atEventSequence: "1",
        perspectiveSeatId: SEAT,
        everyHand: false,
      }),
    ).toBeNull();
  });
});

describe("a game's steps", () => {
  const line = timeline([
    event(5, null),
    event(1, 1),
    event(2, null),
    event(3, 2),
    event(4, null),
    event(3, 2),
    event(6, 3),
  ]);

  test("are its events in order, each once, with where each turn begins", () => {
    expect(line.steps).toEqual(["1", "2", "3", "4", "5", "6"]);
    expect(line.turnStarts).toEqual([0, 2, 5]);
  });

  test("jump to the next turn, or to the end after the last", () => {
    expect(nextTurn(line, 0)).toBe(2);
    expect(nextTurn(line, 3)).toBe(5);
    expect(nextTurn(line, 5)).toBe(5);
  });

  test("jump back to the turn's start, or to the turn before from its start", () => {
    expect(previousTurn(line, 4)).toBe(2);
    expect(previousTurn(line, 2)).toBe(0);
    expect(previousTurn(line, 0)).toBe(0);
  });
});
