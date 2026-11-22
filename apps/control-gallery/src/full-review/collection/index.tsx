import { lazy } from "react";
import type { ScreenProps } from "../../review-types";
import { CardBoard, CardLibrary } from "./Cards";
import {
  DeckLibrary,
  LibraryBoard,
  PackLibrary,
  PackOpening,
  PackBoard,
} from "./Library";
import { Lobby } from "./Lobby";
import { ImportDeck } from "./ImportDeck";
import { AccountSettings, CosmeticScreen } from "./Settings";
import { readDeckLaunch, readShelfContext } from "./deck-library";
import "./collection.css";

const Builder = lazy(() =>
  import("./Builder").then((m) => ({ default: m.Builder })),
);
const BuilderBoard = lazy(() =>
  import("./Builder").then((m) => ({ default: m.BuilderBoard })),
);

export default function CollectionScreen({
  section,
  view,
  go,
  data,
}: ScreenProps) {
  const id = view.id;
  let body;
  switch (section) {
    case "deck-library":
      body = ["DL-04", "DL-06"].includes(id) ? (
        <LibraryBoard id={id} go={go} />
      ) : (
        <DeckLibrary
          id={id}
          go={go}
          launch={readDeckLaunch(data) ?? readShelfContext(data)}
        />
      );
      break;
    case "collection-cards":
      body =
        Number(id.slice(-2)) < 4 ? (
          <CardLibrary id={id} go={go} />
        ) : (
          <CardBoard id={id} go={go} />
        );
      break;
    case "collection-packs":
      body = ["CP-02", "CP-06", "CP-09"].includes(id) ? (
        <PackBoard id={id} go={go} />
      ) : Number(id.slice(-2)) > 10 ? (
        <PackOpening id={id} go={go} />
      ) : (
        <PackLibrary id={id} go={go} />
      );
      break;
    case "deck-builder":
      body = ["DB-01", "DB-02", "DB-07"].includes(id) ? (
        <Builder id={id} go={go} launch={readDeckLaunch(data)} />
      ) : (
        <BuilderBoard id={id} go={go} />
      );
      break;
    case "import-deck":
      body = <ImportDeck id={id} go={go} origin={readShelfContext(data)} />;
      break;
    case "account-settings":
      body = <AccountSettings id={id} go={go} />;
      break;
    case "lobby-details-2026-11-03":
      body = <Lobby id={id} go={go} launch={readDeckLaunch(data)} />;
      break;
    case "default-settings":
    case "cosmetics":
      body = <CosmeticScreen id={id} go={go} />;
      break;
    default:
      return null;
  }
  return (
    <div className="cs-scene" key={id}>
      {body}
    </div>
  );
}
