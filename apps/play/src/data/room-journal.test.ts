import { describe, expect, test } from "bun:test";
import { isAccountPayloadKey } from "./account-storage";
import { roomJournalKey } from "../lobby/room-intents";

const origin = "https://api.example";
const account = "00000000-0000-4000-8000-000000000002";
const room = "00000000-0000-4000-8000-000000000040";

describe("room journals", () => {
  test("every room action's journal belongs to the account, so deletion erases it", () => {
    expect(
      isAccountPayloadKey(
        roomJournalKey(origin, account, "create", null),
        origin,
        account,
      ),
    ).toBe(true);
    for (const kind of [
      "select_deck",
      "leave",
      "ready",
      "unready",
      "force_start",
      "add_ai",
      "bootstrap_ai",
      "remove_ai",
      "configure",
    ] as const)
      expect(
        isAccountPayloadKey(
          roomJournalKey(origin, account, kind, room),
          origin,
          account,
        ),
      ).toBe(true);
  });
  test("a malformed or foreign key is not taken for one", () => {
    expect(
      isAccountPayloadKey(
        `${roomJournalKey(origin, account, "ready", room)}:extra`,
        origin,
        account,
      ),
    ).toBe(false);
    expect(
      isAccountPayloadKey(
        roomJournalKey(origin, account, "ready", "not-a-room"),
        origin,
        account,
      ),
    ).toBe(false);
    expect(
      isAccountPayloadKey(
        roomJournalKey(origin, account, "create", room),
        origin,
        account,
      ),
    ).toBe(false);
    expect(
      isAccountPayloadKey(
        roomJournalKey(
          origin,
          "00000000-0000-4000-8000-000000000003",
          "ready",
          room,
        ),
        origin,
        account,
      ),
    ).toBe(false);
  });
});
