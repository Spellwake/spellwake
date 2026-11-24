import type {
  PlayerCommand,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import type { SourceGap } from "../decks/deck-sources";
import type { CastOffer } from "./cast-draft";
import type { PrintedModes } from "./printed-modes";

export type MatchActionCommand = PlayerCommand["action"];

export type MatchChatEntry = {
  id: string;
  senderId: string;
  senderName: string;
  body: string;
  sentAt: string;
  isMine: boolean;
};

export type MatchConnectionState =
  "connecting" | "live" | "reconnecting" | "ended";

/** How a finished game came out for the person looking at it. */
export type GameResult = {
  outcome: "won" | "lost" | "draw" | "no-result";
  /** Why the game ended, where the game says. */
  reason: string | null;
  /** The seats that won, for the final standings. */
  winnerSeatIds: readonly string[];
};

export type MatchSource =
  | { status: SourceGap | "idle" }
  | {
      status: "ready";
      matchId: string | null;
      gameId: string;
      snapshot: PlayerSnapshot;
      seatNames: ReadonlyMap<string, string>;
      artByPrintingId: ReadonlyMap<string, string>;
      /** Card pictures by card name, for a table whose cards have no printing. */
      faceByName?: ReadonlyMap<string, string>;
      /** Where a modal card's modes are printed, by card name, for a table whose cards have no printing. */
      printedModesByName?: ReadonlyMap<string, PrintedModes>;
      /**
       * The cards the game offers to cast now. Absent where the game does not
       * say, which is every live match until the player view carries offers.
       */
      castOffers?: readonly CastOffer[];
      connectionState: MatchConnectionState;
      /** Set once the game is over. Absent or null while it is being played. */
      result?: GameResult | null;
      commandPending: boolean;
      commandError: string | null;
      dismissCommandError: () => void;
      sendAction: (action: MatchActionCommand) => Promise<void>;
      chooseOpening: ((playFirst: boolean) => Promise<void>) | null;
      chat: {
        messages: readonly MatchChatEntry[];
        send: ((body: string) => Promise<boolean>) | null;
      };
      onLeave: () => void;
    };

export type ReadyMatch = Extract<MatchSource, { status: "ready" }>;
