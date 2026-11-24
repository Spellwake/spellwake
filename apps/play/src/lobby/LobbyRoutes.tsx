import { useCallback, useMemo, type ReactElement } from "react";
import { useLocation, useNavigate, useSearchParams } from "react-router-dom";
import { useAccount } from "../auth/AccountProvider";
import { readInvite } from "../data/room-invite";
import type { PlayMode } from "../decks/deck-sources";
import { LobbyPage } from "../routes/lobby/LobbyPage";
import { useConnectedLobby } from "./connected-lobby";
import { useDemoLobby } from "./demo-lobby";

/** Where a mode's lobby returns to. */
export const shelfPath = (mode: PlayMode): string =>
  mode === "open" ? "/open-play" : "/collection/decks";
export const lobbyPath = (mode: PlayMode): string =>
  mode === "open" ? "/open-play/lobby" : "/collection/lobby";

const useIsPreview = (): boolean =>
  useAccount().state.status === "unconfigured";

function PreviewLobby({ mode }: { mode: PlayMode }): ReactElement {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const back = useCallback(
    () => void navigate(shelfPath(mode)),
    [navigate, mode],
  );
  const roomId = params.get("room");
  const started = useCallback(
    () =>
      void navigate(
        roomId ? `/match?room=${encodeURIComponent(roomId)}` : "/match",
      ),
    [navigate, roomId],
  );
  const source = useDemoLobby(mode, params.get("deck"), back, started);
  return <LobbyPage source={source} onBack={back} />;
}

function AccountLobby({ mode }: { mode: PlayMode }): ReactElement {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const back = useCallback(
    () => void navigate(shelfPath(mode)),
    [navigate, mode],
  );
  const roomId = params.get("room");
  const started = useCallback(
    () =>
      void navigate(
        roomId ? `/match?room=${encodeURIComponent(roomId)}` : "/match",
      ),
    [navigate, roomId],
  );
  // A new room replaces the deck in the address, so a reload returns to the room.
  const onRoom = useCallback(
    (roomId: string) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);
          next.set("room", roomId);
          return next;
        },
        { replace: true },
      ),
    [setParams],
  );
  // An invitation link carries its token in the fragment. Once used it is
  // dropped from the address, so a reload or a shared screen never repeats it.
  const location = useLocation();
  const token = readInvite(location)?.token ?? null;
  const forgetInvite = useCallback(
    () =>
      void navigate(
        { pathname: location.pathname, search: location.search },
        { replace: true },
      ),
    [navigate, location.pathname, location.search],
  );
  const invitation = useMemo(
    () => (token ? { token, onUsed: forgetInvite } : null),
    [token, forgetInvite],
  );
  const source = useConnectedLobby(
    mode,
    params.get("room"),
    params.get("deck"),
    onRoom,
    back,
    started,
    invitation,
  );
  return <LobbyPage source={source} onBack={back} />;
}

/**
 * A mode's lobby. A build that names an application API uses the account's
 * room; only a build with none uses the local preview room.
 */
export function LobbyRoute({ mode }: { mode: PlayMode }): ReactElement {
  return useIsPreview() ? (
    <PreviewLobby mode={mode} />
  ) : (
    <AccountLobby mode={mode} />
  );
}
