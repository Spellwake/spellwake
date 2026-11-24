import type { ReactElement } from "react";
import { parseAsString, useQueryState } from "nuqs";
import { useNavigate, useParams } from "react-router-dom";
import { useAccount } from "../auth/AccountProvider";
import { lobbyPath } from "../lobby/LobbyRoutes";
import { useConnectedFindMatch } from "../play/connected-find-match";
import { useConnectedQuests } from "../play/connected-quests";
import { useDemoFindMatch } from "../play/demo-find-match";
import { useDemoQuests } from "../play/demo-quests";
import { DeckBuilderPage } from "../routes/builder/DeckBuilderPage";
import {
  DeckShelfPage,
  type PlayProps,
} from "../routes/collection/DeckShelfPage";
import { PlayDialog } from "../routes/collection/PlayDialog";
import {
  useConnectedDeckBuilder,
  useConnectedDeckShelf,
} from "./connected-sources";
import type { PlayMode } from "./deck-sources";
import { useDemoDeckBuilder, useDemoDeckShelf } from "./demo-sources";

/**
 * Chooses where the deck screens get their data. A build that names an
 * application API always uses the account, and shows the account's own state
 * when it cannot be read. Only a build with no API configured uses the local
 * preview, so a deployment can never fall back to preview data by accident.
 */
const useIsPreview = (): boolean =>
  useAccount().state.status === "unconfigured";

function AccountPlay({
  mode,
  deck,
  onFriends,
  onClose,
}: PlayProps & { mode: PlayMode }): ReactElement {
  const navigate = useNavigate();
  const find = useConnectedFindMatch(
    mode,
    deck,
    (found) =>
      // A seat that could not be readied is shown its room, where it can be.
      void navigate(
        found.to === "table"
          ? `/match?room=${found.roomId}`
          : `${lobbyPath(mode)}?room=${found.roomId}`,
      ),
  );
  return (
    <PlayDialog
      deck={deck}
      mode={mode}
      find={find}
      onFriends={onFriends}
      onClose={onClose}
    />
  );
}
const DEMO_SEARCHES = ["waits", "refused", "single-only", "none"] as const;
function PreviewPlay({
  mode,
  deck,
  onFriends,
  onClose,
}: PlayProps & { mode: PlayMode }): ReactElement {
  const navigate = useNavigate();
  // `?search=` chooses how the preview's search behaves; it finds a match otherwise.
  const [param] = useQueryState("search", parseAsString);
  const behaviour = DEMO_SEARCHES.find((name) => name === param) ?? "finds";
  const find = useDemoFindMatch(deck, behaviour, () => void navigate("/match"));
  return (
    <PlayDialog
      deck={deck}
      mode={mode}
      find={find}
      onFriends={onFriends}
      onClose={onClose}
    />
  );
}

function AccountShelf({ mode }: { mode: PlayMode }): ReactElement {
  return (
    <DeckShelfPage
      mode={mode}
      source={useConnectedDeckShelf(mode)}
      play={(props) => <AccountPlay mode={mode} {...props} />}
      quests={useConnectedQuests()}
    />
  );
}
const DEMO_QUESTS = ["one", "none", "refused"] as const;
function PreviewShelf({ mode }: { mode: PlayMode }): ReactElement {
  // `?quests=` chooses how the preview's daily quests behave.
  const [questsParam] = useQueryState("quests", parseAsString);
  return (
    <DeckShelfPage
      mode={mode}
      source={useDemoDeckShelf(mode)}
      play={(props) => <PreviewPlay mode={mode} {...props} />}
      quests={useDemoQuests(
        DEMO_QUESTS.find((name) => name === questsParam) ?? "three",
      )}
    />
  );
}
export function DeckShelfRoute({ mode }: { mode: PlayMode }): ReactElement {
  return useIsPreview() ? (
    <PreviewShelf mode={mode} />
  ) : (
    <AccountShelf mode={mode} />
  );
}

function AccountBuilder({ deckId }: { deckId: string | undefined }) {
  return <DeckBuilderPage source={useConnectedDeckBuilder(deckId)} />;
}
function PreviewBuilder({ deckId }: { deckId: string | undefined }) {
  return <DeckBuilderPage source={useDemoDeckBuilder(deckId)} />;
}
export function DeckBuilderRoute(): ReactElement {
  const { deckId } = useParams();
  return useIsPreview() ? (
    <PreviewBuilder deckId={deckId} />
  ) : (
    <AccountBuilder deckId={deckId} />
  );
}
