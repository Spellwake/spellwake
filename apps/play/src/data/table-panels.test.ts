import { describe, expect, test } from "bun:test";
import { playerSnapshotSchema } from "@spellwake/application-contracts";
import type { MatchChatEntry } from "../match/match-sources";
import {
  previewConcede,
  previewScene,
  previewSnapshot,
} from "../match/preview-table";
import { seatBoard } from "../match/table-model";
import {
  DEFAULT_PREFERENCES,
  hasUnseen,
  readPreferences,
  settleDraft,
} from "../match/table-panels";

const message = (id: string, isMine = false): MatchChatEntry => ({
  id,
  senderId: isMine ? "me" : "alex",
  senderName: isMine ? "You" : "Alex",
  body: "Hello.",
  sentAt: "2026-11-22T18:00:00.000Z",
  isMine,
});

describe("a sent draft", () => {
  test("is cleared only when the same text was accepted (CM-04)", () => {
    expect(settleDraft("Good game.", "Good game.", true)).toBe("");
  });
  test("is kept when the table refuses it (CM-05C)", () => {
    expect(settleDraft("Good game.", "Good game.", false)).toBe("Good game.");
  });
  test("is kept when more was typed while it was being sent", () => {
    expect(settleDraft("Good game. Again?", "Good game.", true)).toBe(
      "Good game. Again?",
    );
  });
});

describe("the unread dot", () => {
  const history = [message("1"), message("2", true), message("3")];
  test("shows for another player's message that has not been on screen", () => {
    expect(hasUnseen(history, null)).toBe(true);
    expect(hasUnseen(history, "2")).toBe(true);
  });
  test("clears once the newest message has been seen", () => {
    expect(hasUnseen(history, "3")).toBe(false);
  });
  test("never shows for your own messages", () => {
    expect(hasUnseen([message("1"), message("2", true)], "1")).toBe(false);
  });
  test("is absent with no messages, or when what was seen is gone", () => {
    expect(hasUnseen([], null)).toBe(false);
    expect(hasUnseen(history, "elsewhere")).toBe(false);
  });
});

describe("stored settings", () => {
  test("come back as they were kept", () => {
    const kept = { master: 60, music: 0, effects: 40, autoTap: false };
    expect(readPreferences(kept)).toEqual(kept);
  });
  test("fall back for anything missing, damaged or out of range", () => {
    expect(readPreferences(null)).toEqual(DEFAULT_PREFERENCES);
    expect(readPreferences("loud")).toEqual(DEFAULT_PREFERENCES);
    expect(
      readPreferences({ master: 250, music: -3, effects: "x", autoTap: 1 }),
    ).toEqual({
      master: 100,
      music: 0,
      effects: DEFAULT_PREFERENCES.effects,
      autoTap: DEFAULT_PREFERENCES.autoTap,
    });
  });
});

describe("conceding in the preview (CM-09)", () => {
  const before = previewSnapshot();
  const after = previewConcede(before);
  const you = before.viewerSeatId;

  test("leaves a snapshot the contract accepts", () => {
    expect(playerSnapshotSchema.safeParse(after).error?.issues ?? []).toEqual(
      [],
    );
    expect(previewScene("watching")).toEqual(after);
  });

  test("puts the viewer out and empties their seat", () => {
    expect(after.players.find((player) => player.seatId === you)).toMatchObject(
      { eliminated: true, handCount: "0", libraryCount: "0" },
    );
    const board = seatBoard(after, you);
    expect([
      board.front.length,
      board.lands.length,
      board.others.length,
      board.hand.length,
      board.graveyard.length,
      board.libraryCount,
    ]).toEqual([0, 0, 0, 0, 0, 0]);
  });

  test("passes a turn that was theirs, and leaves everyone else's table alone", () => {
    expect(after.turn?.activeSeatIds).not.toContain(you);
    expect(after.turn?.prioritySeatIds).not.toContain(you);
    expect(after.decisions).toEqual([]);
    const others = (snapshot: typeof before) =>
      snapshot.visibleObjects.filter((object) => object.ownerSeatId !== you);
    expect(others(after)).toEqual(others(before));
  });
});
