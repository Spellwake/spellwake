import type {
  PlayerObject,
  PlayerSnapshot,
} from "@spellwake/application-contracts";
import type { SourceGap } from "../decks/deck-sources";

/** A game of the match being replayed. */
export type ReplayGame = { id: string; label: string };

export type ReplaySource =
  | { status: SourceGap | "missing" }
  | {
      status: "ready";
      /** "vs Alex · Modern". */
      title: string;
      games: readonly ReplayGame[];
      gameIndex: number;
      chooseGame: (index: number) => void;
      seatNames: ReadonlyMap<string, string>;
      faceOf: (object: PlayerObject) => string | undefined;
      /** The table after the current step; null while it is read. */
      snapshot: PlayerSnapshot | null;
      step: number;
      /** How many steps the game has; zero while they are read. */
      steps: number;
      go: (step: number) => void;
      nextTurn: () => void;
      previousTurn: () => void;
      /** Whether every hand is shown; null where only the player's own seat can be. */
      everyHand: boolean | null;
      setEveryHand: (on: boolean) => void;
      /** Set when part of the replay could not be read. */
      notice: string | null;
    };
