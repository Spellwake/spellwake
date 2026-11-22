/**
 * Owned sealed packs and the one opening in progress, as the Packs screen
 * reads them from the account or the local preview.
 */

/** One product of owned packs. */
export type OwnedPack = {
  /** The owned pack asset an opening names. */
  id: string;
  /** The set's name, shown under the pack and offered by the set filter. */
  setName: string;
  /** The product's full name, shown over an opening, such as "Foundations Play Booster". */
  product: string;
  /** Searched with the names. */
  setCode: string | null;
  owned: number;
  /** Reserved in a trade: still owned, not available to open. */
  held: number;
  available: number;
};

/** A card an opening gave. Duplicates arrive as separate cards. */
export type PackCard = {
  key: string;
  name: string;
  /** The printed face, or null where no artwork is available. */
  faceUrl: string | null;
  finish: string;
};

/**
 * Where the opening stands. "opening" is the first attempt in flight;
 * "unconfirmed" means its outcome is unknown and only Check opening may
 * settle it; "checking" is that check in flight; "loading" means the opening
 * was accepted and its cards are being read; "opened" holds the cards.
 */
export type OpeningStage =
  "opening" | "unconfirmed" | "checking" | "loading" | "opened";
export type Opening = {
  packId: string;
  product: string;
  stage: OpeningStage;
  cards: readonly PackCard[] | null;
};

export type PacksGap =
  "loading" | "signed-out" | "unregistered" | "unavailable";
export type PacksSource =
  | { status: PacksGap }
  | {
      status: "ready";
      /** In the order the library keeps them. */
      packs: readonly OwnedPack[];
      isAppending: boolean;
      /** True when the first read failed, so the library is not known to be empty. */
      isUnread: boolean;
      /** A passive message to say once, such as a read or opening that failed. */
      notice: string | null;
      dismissNotice: () => void;
      /** False where the account may not open packs. */
      canOpen: boolean;
      opening: Opening | null;
      /** Starts one opening of one pack. Nothing is asked first. */
      open: (packId: string) => void;
      /** Asks again about an opening whose outcome is unknown, never sending a new one. */
      checkOpening: () => void;
      /** Closes a resolved opening. Its cards are owned whether or not they were revealed. */
      closeOpening: () => void;
    };

/**
 * The count a pack shows: only packs that can be used now. Packs in a trade
 * or listed are not mentioned; "3 owned, 1 in a trade" reads as "2 packs".
 */
export const packsLabel = (count: number): string =>
  `${count} ${count === 1 ? "pack" : "packs"}`;
