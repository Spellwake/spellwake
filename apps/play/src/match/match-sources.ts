import type {
  PlayerCommand,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import type { SourceGap } from "../decks/deck-sources";
import type { CastOffer } from "./cast-draft";
import type { PrintedModes } from "./printed-modes";
import type { SideboardCard } from "./sideboard";

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

/**
 * Where a match of several games stands once one of its games has ended.
 * Absent for a single game.
 */
export type MatchSeries = {
  /** The finished game's place in the match, from 1. */
  gameNumber: number;
  /** Games won so far. Null at a table of more than two, where no pair of numbers says it. */
  score: { you: number; them: number } | null;
  next:
    | {
        /** The match is decided; this is how it came out for the viewer. */
        kind: "over";
        outcome: GameResult["outcome"];
      }
    | {
        /**
         * The next game waits on this player, until the time runs out and
         * the game starts with the same deck.
         */
        kind: "deciding";
        deadlineAt: string | null;
        /** Starts the next game with the same deck. Null where this table cannot say which deck that is. */
        ready: (() => void) | null;
        readyPending: boolean;
        /** Set when the answer was not taken; the time still runs. */
        readyFailed: boolean;
        /** Changing the deck for the next game. Null where this table cannot read the deck. */
        sideboard: Sideboard | null;
      }
    | {
        /** This player has answered; the next game waits on the others or is being dealt. */
        kind: "waiting";
      };
};

/**
 * Sideboarding between games: moving cards between the main deck and the
 * sideboard. The cards stay the same; only where each copy sits changes.
 */
export type Sideboard = {
  /** The deck as the next game would use it; null while it is read. */
  cards: readonly SideboardCard[] | null;
  /** The fewest cards the main deck may hold: as many as it began the match with. */
  minimumMain: number;
  /** Saves the list and starts the next game with it. */
  use: (cards: readonly SideboardCard[]) => void;
  pending: boolean;
  /** Why the last attempt did not go through. */
  failure: string | null;
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
       * The cards the game offers to cast now, derived from server-authored
       * `PlayerSnapshot.castOffers` in live matches or preview offers in local mode.
       */
      castOffers?: readonly CastOffer[];
      connectionState: MatchConnectionState;
      /** Set once the game is over. Absent or null while it is being played. */
      result?: GameResult | null;
      /** Set with `result` where the game was one of a match of several. */
      series?: MatchSeries | null;
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
