import { describe, expect, test } from "bun:test";
import { inviteAddress, inviteLocation, readInvite } from "./room-invite";

const roomId = "0b8f6c1e-2a4d-4e7b-9c3a-5d6e7f809a1b";
const token = `${"A".repeat(42)}Q`;

describe("room invitations", () => {
  test("an address names the room and keeps the token in the fragment", () => {
    const address = inviteAddress("https://app.spellwake.com", "/", {
      mode: "open",
      roomId,
      token,
    });
    expect(address).toBe(
      `https://app.spellwake.com/open-play/lobby?room=${roomId}#invite=${token}`,
    );
    expect(new URL(address).search).not.toContain(token);
  });

  test("an address under a base path keeps the base", () => {
    expect(
      inviteAddress("http://127.0.0.1:6242", "/play/", {
        mode: "collection",
        roomId,
        token,
      }),
    ).toBe(
      `http://127.0.0.1:6242/play/collection/lobby?room=${roomId}#invite=${token}`,
    );
  });

  test("a place in the app reads back to the same invitation", () => {
    const invite = { mode: "open" as const, roomId, token };
    const url = new URL(inviteLocation(invite), "https://invite.invalid");
    expect(readInvite(url)).toEqual(invite);
  });

  test("anything short of a whole invitation reads as none", () => {
    const at = (pathname: string, search: string, hash: string) =>
      readInvite({ pathname, search, hash });
    expect(at("/open-play/lobby", `?room=${roomId}`, "")).toBeNull();
    expect(at("/open-play/lobby", "", `#invite=${token}`)).toBeNull();
    expect(
      at("/open-play/lobby", `?room=${roomId}`, "#invite=short"),
    ).toBeNull();
    expect(
      at("/open-play/lobby", "?room=not-a-room", `#invite=${token}`),
    ).toBeNull();
    expect(
      at("/market/listings", `?room=${roomId}`, `#invite=${token}`),
    ).toBeNull();
  });
});
