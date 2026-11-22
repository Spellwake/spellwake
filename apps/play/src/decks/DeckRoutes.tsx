import type { ReactElement } from "react";
import { useParams } from "react-router-dom";
import { useAccount } from "../auth/AccountProvider";
import { DeckBuilderPage } from "../routes/builder/DeckBuilderPage";
import { DeckShelfPage } from "../routes/collection/DeckShelfPage";
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

function AccountShelf({ mode }: { mode: PlayMode }): ReactElement {
  return <DeckShelfPage mode={mode} source={useConnectedDeckShelf(mode)} />;
}
function PreviewShelf({ mode }: { mode: PlayMode }): ReactElement {
  return <DeckShelfPage mode={mode} source={useDemoDeckShelf(mode)} />;
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
