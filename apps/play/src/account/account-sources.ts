import type { Outcome, SourceGap } from "../decks/deck-sources";
import type { PortraitCrop } from "./portrait-crop";

/**
 * What Account settings and the public profile ask of their data. Each screen
 * takes one of these and nothing else, so it runs the same on the signed-in
 * account and on the local preview. A source says plainly what it cannot do:
 * a command it lacks is null, and a fact it cannot read is "unknown" or null,
 * never a guess.
 */

/** One piece of catalogue artwork, cropped to a circle. */
export type Portrait = { artUrl: string; crop: PortraitCrop };

export type BadgeChoice = "none" | "supporter";

/** Artwork a person can choose for their picture. Any card, owned or not. */
export type ArtChoice = {
  id: string;
  name: string;
  /** Null where the catalogue does not credit the artwork. */
  artist: string | null;
  artUrl: string;
};

export type ArtSearch = {
  query: string;
  setQuery: (query: string) => void;
  state: "loading" | "ready" | "failed";
  results: readonly ArtChoice[];
};

export type PurchaseStatus =
  "Creating" | "Pending" | "Paid" | "Canceled" | "Refunded" | "Reversed";

export type PurchaseRow = {
  id: string;
  item: string;
  /** When the purchase was made, as an instant. */
  date: string;
  /** The price as charged, such as "$2.99 USD". */
  amount: string;
  status: PurchaseStatus;
};

export type PurchaseHistory = {
  /** "failed" is a list that could not be read. It is never shown as empty. */
  state: "loading" | "ready" | "failed";
  rows: readonly PurchaseRow[];
  /** Asks for the next page. Null once every purchase is loaded. */
  loadMore: (() => void) | null;
};

/**
 * - `unlinked`: no Patreon account is associated.
 * - `linked`: linked, and whether there is a current membership is not known.
 * - `inactive`: linked, with no current membership.
 * - `active`: linked, and a current supporter.
 * - `unknown`: the source cannot tell which of these is true.
 */
export type PatreonLink =
  "unlinked" | "linked" | "inactive" | "active" | "unknown";

export type AccountSettingsSource =
  | { status: SourceGap }
  | {
      status: "ready";
      /** Fixed when the account was created. */
      username: string;
      portrait: Portrait | null;
      badge: BadgeChoice;
      /** Choosing a picture. Null where artwork cannot be searched or saved. */
      picture: {
        search: ArtSearch;
        save: (art: ArtChoice, crop: PortraitCrop) => Promise<Outcome>;
      } | null;
      /** Choosing a badge. Null where the choice cannot be saved. */
      badges: {
        canUseSupporter: boolean;
        save: (badge: BadgeChoice) => Promise<Outcome>;
      } | null;
      patreon: {
        link: PatreonLink;
        /** Opens Patreon's own authorisation. Null where linking is not offered. */
        connect: (() => Promise<Outcome>) | null;
        /** Opens Patreon's own membership page. Null where it cannot be opened. */
        manage: (() => Promise<Outcome>) | null;
        /** True while a return from Patreon is being finished. */
        isLinking: boolean;
        /** A link that has just failed, to be said once and then dismissed. */
        problem: string | null;
        dismissProblem: () => void;
      };
      billing: {
        /** Opens the hosted billing page. Null where there is none to open. */
        manage: (() => Promise<Outcome>) | null;
        purchases: PurchaseHistory;
      };
      deletion: {
        /** "requested" once a deletion has been accepted. It cannot be withdrawn here. */
        state: "none" | "requested";
        request: () => Promise<Outcome>;
      };
    };

/** What the header and other small surfaces show of the person using the app. */
export type OwnIdentity = { portrait: Portrait | null; badge: BadgeChoice };

export type RankRow = {
  format: string;
  /** Null for a format with a rating and no placed tier yet. */
  tier: { name: string; emblemUrl: string } | null;
  rating: number;
  record: { wins: number; losses: number; draws: number };
};

export type PublicProfileSource =
  /** Nothing is known yet. No identity, rank or action is shown. */
  | { status: "loading" }
  /** The first read failed. */
  | { status: "failed" }
  /** Deleted, private or unknown. No reason is disclosed. */
  | { status: "unavailable" }
  | {
      status: "ready";
      username: string;
      portrait: Portrait | null;
      badge: BadgeChoice;
      /** The season these ranks belong to, such as "Oct–Dec 2026". */
      season: string | null;
      /**
       * This season's public ranks. An empty list is a player with no ranked
       * matches. Null is ranks that could not be read, which is not the same.
       */
      ranks: readonly RankRow[] | null;
      /** True when a later refresh failed and this is the last good read. */
      isStale: boolean;
      /**
       * Opens this person's conversation. Null unless contact is currently
       * permitted, which includes while permission is still being checked.
       */
      message: (() => Promise<Outcome>) | null;
    };
