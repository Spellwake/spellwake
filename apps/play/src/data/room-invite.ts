import {
  identifierSchema,
  invitationTokenSchema,
} from "@spellwake/application-contracts";

/**
 * A friend-game invitation as an address carries it: the room, and the
 * capability that admits whoever holds it. The capability rides in the
 * fragment, which a browser never sends to a server or keeps in a referrer.
 * It is never logged, and is held only until it has been used.
 */
export type RoomInvite = {
  mode: "open" | "collection";
  roomId: string;
  token: string;
};

const LOBBY_PATHS: Record<RoomInvite["mode"], string> = {
  open: "/open-play/lobby",
  collection: "/collection/lobby",
};
const HELD_KEY = "spellwake.room-invite.v1";

/** The invitation's place in the app, for the router. */
export const inviteLocation = (invite: RoomInvite): string =>
  `${LOBBY_PATHS[invite.mode]}?room=${invite.roomId}#invite=${invite.token}`;

/** The whole address a host hands to a friend. */
export function inviteAddress(
  origin: string,
  base: string,
  invite: RoomInvite,
): string {
  return new URL(`${base.replace(/\/$/, "")}${inviteLocation(invite)}`, origin)
    .href;
}

/** An invitation named by a place in the app, or null when it names none. */
export function readInvite(location: {
  pathname: string;
  search: string;
  hash: string;
}): RoomInvite | null {
  const path = location.pathname.replace(/\/+$/, "");
  const mode = (["open", "collection"] as const).find(
    (candidate) => LOBBY_PATHS[candidate] === path,
  );
  const room = identifierSchema.safeParse(
    new URLSearchParams(location.search).get("room"),
  );
  const token = invitationTokenSchema.safeParse(
    new URLSearchParams(location.hash.replace(/^#/, "")).get("invite"),
  );
  return mode && room.success && token.success
    ? { mode, roomId: room.data, token: token.data }
    : null;
}

/** Keeps an invitation in this tab while its bearer signs in. */
export function holdInvite(invite: RoomInvite): void {
  try {
    window.sessionStorage.setItem(HELD_KEY, JSON.stringify(invite));
  } catch {
    // A tab with no storage loses the invitation on sign-in; the link still works.
  }
}

/** The held invitation, which is forgotten as it is taken. */
export function takeHeldInvite(): RoomInvite | null {
  try {
    const raw = window.sessionStorage.getItem(HELD_KEY);
    if (!raw) return null;
    window.sessionStorage.removeItem(HELD_KEY);
    return readInvite(
      new URL(
        inviteLocation(JSON.parse(raw) as RoomInvite),
        "https://invite.invalid",
      ),
    );
  } catch {
    return null;
  }
}
