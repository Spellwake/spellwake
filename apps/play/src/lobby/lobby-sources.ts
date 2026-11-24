import type { BoxAppearance } from "@spellwake/deck-box";
import type { Portrait } from "../account/account-sources";
import type { Outcome, PlayMode } from "../decks/deck-sources";

/**
 * The friend-game lobby as its screen reads it, from the account or the
 * local preview. A control the source cannot carry out is null and is left
 * off the screen. Everything here mirrors the room's authority: the screen
 * never assumes a change succeeded before the source says so.
 */

/** A deck that can sit in a seat: the person's own, or the AI's Random Deck. */
export type LobbyDeck = {
  id: string;
  name: string;
  formatName: string;
  appearance: BoxAppearance;
};
/** The AI's own choice of deck, offered first in the AI's chooser. */
export const RANDOM_DECK = "random";

export type AiTier = "normal" | "advanced";
export type AiDifficulty = AiTier;

/** Daily Advanced AI quota state shown in the lobby when AI seats are present. */
export type LobbyAiQuota = {
  displayUnlimited: boolean;
  dailyLimit: number | null;
  remainingToday: number | null;
  requiredAdvanced: number;
  resetsAt: string;
};
export type LobbySeat = {
  id: string;
  kind: "you" | "person" | "ai" | "open";
  /** Shown as written: "You", a person's name, "AI player" or "Open seat". */
  name: string;
  portrait: Portrait | null;
  isHost: boolean;
  isReady: boolean;
  /**
   * Your own deck, by name. Another person's deck is private: only whether
   * one is chosen, or "private" where even that is not shared. An AI seat
   * names its deck or Random Deck.
   */
  deck: LobbyDeck | "selected" | "random" | "private" | null;
  /** A deck that no longer fits the rules and must be replaced. */
  needsNewDeck: boolean;
  /** Null where difficulty cannot be chosen. */
  difficulty: AiDifficulty | null;
};

/** The room's rules as the summary shows them. */
export type LobbyRules = {
  format: string;
  tableRules: string;
  players: number;
  match: "Single game" | "Best of three";
  planechase: boolean | null;
  influence: string | null;
};
/** What the rules drawer may change. A field with no choices is shown but not editable. */
export type RulesChoices = {
  formats: readonly string[];
  tableRules: readonly string[];
  players: readonly number[];
  matches: readonly LobbyRules["match"][];
  planechase: boolean;
  influences: readonly string[];
};

export type Phase =
  /** Seats may change. */
  | "open"
  /** All seats are taken and every deck fits; the game starts at the deadline. */
  | "countdown"
  /** The start is being committed. Nothing may change. */
  | "starting"
  /** The start was sent but its outcome is not known; only Check status asks. */
  | "start-unconfirmed"
  /** The game began. */
  | "started";

/** Leaving or closing, from the first press to its settled outcome. */
export type Departure =
  | { stage: "confirm" }
  | { stage: "pending" }
  | { stage: "unconfirmed"; isChecking: boolean }
  | { stage: "rejected" };

/** Someone the host can invite. */
export type InviteCandidate = {
  id: string;
  name: string;
  portrait: Portrait | null;
};

export type LobbyGap =
  | "loading"
  | "signed-out"
  | "unregistered"
  /** The host closed the lobby. */
  | "closed"
  /** Every human seat is taken. */
  | "full"
  /** The room does not exist, or this person may not join it. */
  | "unavailable";

export type LobbySource =
  | { status: LobbyGap }
  | {
      status: "ready";
      mode: PlayMode;
      rules: LobbyRules;
      seats: readonly LobbySeat[];
      isHost: boolean;
      /** Your own deck, for the deck panel. */
      yourDeck: LobbyDeck | null;
      phase: Phase;
      /** The countdown's absolute end, in milliseconds, while counting down. */
      deadline: number | null;
      /** A passive message to say once. */
      notice: string | null;
      dismissNotice: () => void;
      /** Decks eligible for the room's format, for Choose Deck. Null while loading. */
      decks: {
        state: "loading" | "ready" | "failed";
        items: readonly LobbyDeck[];
      };
      reloadDecks: () => void;
      chooseDeck: (deckId: string) => Promise<Outcome>;
      setReady: (ready: boolean) => Promise<Outcome>;
      /** Host only, or null. */
      start: (() => Promise<Outcome>) | null;
      checkStart: () => void;
      /** Host only: turn an empty or AI seat into the other kind. */
      setController:
        ((seatId: string, kind: "human" | "ai") => Promise<Outcome>) | null;
      setDifficulty:
        ((seatId: string, difficulty: AiDifficulty) => Promise<Outcome>) | null;
      /** Daily Advanced AI allowance for the host, or null when unavailable. */
      aiQuota?: LobbyAiQuota | null;
      /** Host only: the AI's deck, or RANDOM_DECK. */
      setAiDeck: ((seatId: string, deckId: string) => Promise<Outcome>) | null;
      /** Host only, or null where rules cannot be changed. */
      rulesChoices: RulesChoices | null;
      applyRules: ((next: LobbyRules) => Promise<Outcome>) | null;
      /** Host only, or null where people cannot be invited by name. */
      findPlayers:
        ((query: string) => Promise<readonly InviteCandidate[] | null>) | null;
      invite: ((playerId: string) => Promise<Outcome>) | null;
      /**
       * Host only: makes the address that admits whoever opens it, or null
       * where the source has none to give.
       */
      inviteLink: (() => Promise<Outcome<string>>) | null;
      departure: Departure | null;
      /** Asks to leave, or for the host to close. Opens the confirmation. */
      beginDeparture: () => void;
      cancelDeparture: () => void;
      confirmDeparture: () => void;
      checkDeparture: () => void;
    };

export const MATCH_LABEL: Record<"bo1" | "bo3", LobbyRules["match"]> = {
  bo1: "Single game",
  bo3: "Best of three",
};
/** "Standard · Free-for-All · 4 players" */
export const rulesLine = (rules: LobbyRules): string =>
  `${rules.format} · ${rules.tableRules} · ${rules.players} players`;
/** Whether the host may start now: every seat filled and every deck fits. */
export function canStart(seats: readonly LobbySeat[]): boolean {
  return seats.every(
    (seat) => seat.kind !== "open" && seat.deck !== null && !seat.needsNewDeck,
  );
}
