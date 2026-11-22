import { useCallback, type ReactElement } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { useAccount } from "../auth/AccountProvider";
import { MatchPage } from "../routes/match/MatchPage";
import { useConnectedMatch } from "./connected-match";
import { useDemoMatch } from "./demo-match";

function PreviewMatch(): ReactElement {
  const navigate = useNavigate();
  const leave = useCallback(
    () => void navigate("/collection/decks"),
    [navigate],
  );
  return <MatchPage source={useDemoMatch(leave)} />;
}

function ConnectedMatch(): ReactElement {
  const navigate = useNavigate();
  const { gameId } = useParams();
  const [params] = useSearchParams();
  const leave = useCallback(
    () => void navigate("/collection/decks"),
    [navigate],
  );
  const source = useConnectedMatch(
    {
      gameId: gameId ?? params.get("game"),
      contextId: params.get("context"),
      matchId: params.get("match"),
      roomId: params.get("room"),
    },
    leave,
  );
  return <MatchPage source={source} />;
}

/** Live match route. Uses the local preview match only in an unconfigured build. */
export function MatchRoute(): ReactElement {
  return useAccount().state.status === "unconfigured" ? (
    <PreviewMatch />
  ) : (
    <ConnectedMatch />
  );
}
