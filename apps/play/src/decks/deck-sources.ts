import type { BoxAppearance } from "@spellwake/deck-box";
import type {
  AppearanceOptions,
  DefaultSelection,
} from "../cosmetics/cosmetic-sources";
import type { BuilderCard, FormatLegality } from "../data/builder-catalogue";
import type { CardRef, DeckLine } from "../data/deck-draft";

/**
 * What the deck shelf and the deck builder ask of their data. A screen takes
 * one of these and nothing else, so the same screen runs on the signed-in
 * account or on the local preview. Each source says plainly what it cannot do:
 * a missing command is null, and missing card facts are undefined.
 */

/** Open Play uses every card. Collection Play uses owned cards. Their decks never mix. */
export type PlayMode = "open" | "collection";

export type Outcome<Value = undefined> =
  | { status: "success"; value: Value }
  /** `message` is ready to show to the player. */
  | { status: "fail"; message: string };
export const succeeded = <Value>(value: Value): Outcome<Value> => ({
  status: "success",
  value,
});
export const failed = (message: string): Outcome<never> => ({
  status: "fail",
  message,
});

export type FormatChoice = { id: string; name: string };

/**
 * Why a source has nothing to show yet. "unregistered" is a signed-in person
 * who has not finished creating their account.
 */
export type SourceGap =
  "loading" | "signed-out" | "unregistered" | "unavailable";

export type ShelfDeck = {
  id: string;
  name: string;
  format: FormatChoice;
  appearance: BoxAppearance;
};

export type DeckShelfSource =
  | { status: SourceGap }
  | {
      status: "ready";
      decks: readonly ShelfDeck[];
      formats: readonly FormatChoice[];
      /** The format last chosen on this shelf, for the next new deck. */
      lastFormatId: string | null;
      rememberFormat: (formatId: string) => void;
      /** Saves an empty Untitled deck. The caller opens it in the builder. */
      createDeck: (formatId: string) => Promise<Outcome<{ deckId: string }>>;
      /**
       * Saves one Untitled deck holding the cards a list names that are
       * found. The caller opens it in the builder. `message` on failure is
       * ready to show, and tells a failed import from an unconfirmed one.
       */
      importDeck: (
        formatId: string,
        text: string,
      ) => Promise<Outcome<{ deckId: string }>>;
    };

export type DeckDraft = {
  name: string;
  formatId: string;
  lines: DeckLine[];
  /** The card shown on the deck box, by name. Null for plain leather. */
  faceCard: string | null;
  /** The deck's sleeves, card back, dice, playmat and pet. Null where the source keeps none. */
  appearance: DefaultSelection | null;
};

/** A card offered by search. `facts` is absent where the source knows the card by name only. */
export type SearchCard = {
  name: string;
  card?: CardRef;
  customCardRevisionId?: string;
  facts?: BuilderCard;
};

export type CardSearch = {
  query: string;
  setQuery: (query: string) => void;
  /** Colour and type filters, or null where the source cannot filter by them. */
  filters: {
    color: string;
    setColor: (color: string) => void;
    type: string;
    setType: (type: string) => void;
  } | null;
  /** "idle" means the source needs something typed before it searches. */
  state: "idle" | "loading" | "ready" | "failed";
  results: readonly SearchCard[];
};

export type DeckBuilderSource =
  | { status: SourceGap | "missing" }
  | {
      status: "ready";
      /** The deck as last saved. A new object each time a save lands. */
      deck: {
        id: string;
        name: string;
        formatId: string;
        mode: PlayMode;
        lines: readonly DeckLine[];
        appearance: DefaultSelection | null;
        faceCard: string | null;
      };
      /** Whether a face card can be chosen and kept. False where the source has nowhere to keep one. */
      canChooseFaceCard: boolean;
      /** Whether a card has official art to show on the deck box. */
      showsOnBox: (name: string) => boolean;
      formats: readonly FormatChoice[];
      /** What the deck's appearance may be chosen from. Null where it cannot be changed. */
      appearanceOptions: AppearanceOptions | null;
      /** What is printed on a card, where the source has it. */
      facts: (name: string) => BuilderCard | undefined;
      /**
       * Whether a card may be in a deck of a format. A source that cannot
       * check answers null, and the builder then neither blocks nor marks cards.
       */
      legality: ((name: string, format: FormatChoice) => FormatLegality) | null;
      /** The set and collector number of a row's printing, for export. */
      printingOf: (line: DeckLine) => { set: string; number: string } | null;
      search: CardSearch;
      hasCardsFailed: boolean;
      save: (draft: DeckDraft) => Promise<Outcome>;
      /** Null where the source has no such command. */
      duplicate: ((draft: DeckDraft) => Promise<Outcome>) | null;
      /** Null where the source has no such command. */
      remove: (() => Promise<Outcome>) | null;
    };
