/**
 * Market Supply, as the screen reads it from the local preview or the
 * account: every set as a gallery of pack products, each bought at its
 * current sequential price, and the vote for the next supply drop. A source
 * says only what it knows; a quote or a ballot it cannot read is shown as
 * unknown, never as sold out or closed.
 */

export type Availability = "all" | "available" | "sold-out" | "unavailable";
export const AVAILABILITY_LABELS: Record<Availability, string> = {
  all: "All",
  available: "Available",
  "sold-out": "Sold out",
  unavailable: "Unavailable",
};

/** The terms one quote gives for a quantity. */
export type QuoteTerms = {
  quantity: number;
  remaining: number;
  /** The exact total for the quantity, or null when not enough remain. */
  total: number | null;
  /** What a purchase must name back, as the service writes it. */
  policyId: string;
  revision: string;
  totalText: string | null;
};

/** One gallery tile: a qualified pack product, or a set with none. */
export type SupplyTile =
  | {
      kind: "product";
      /** The recipe a quote and a purchase name. */
      id: string;
      setId: string;
      setName: string;
      setCode: string | null;
      /** "Play Booster": the product's name without its set. */
      product: string;
      /** "Foundations Play Booster". */
      fullName: string;
      /** Null while the quantity-one quote is being read, or failed. */
      quote: QuoteTerms | null;
      quoteState: "checking" | "ready" | "failed";
    }
  | {
      kind: "set";
      id: string;
      setName: string;
      setCode: string | null;
    };

/** A tile's availability as the filter and the tile say it. Unknown quotes are not Sold out. */
export function tileAvailability(
  tile: SupplyTile,
): "checking" | "available" | "sold-out" | "unavailable" {
  if (tile.kind === "set") return "unavailable";
  if (tile.quoteState !== "ready" || !tile.quote) return "checking";
  return tile.quote.remaining > 0 ? "available" : "sold-out";
}

/** Search matches set names, set codes and product names; Availability applies as chosen. */
export function matchTiles(
  tiles: readonly SupplyTile[],
  query: string,
  availability: Availability,
): SupplyTile[] {
  const needle = query.trim().toLowerCase();
  return tiles.filter((tile) => {
    const text = [
      tile.setName,
      tile.setCode ?? "",
      tile.kind === "product" ? tile.fullName : "",
    ]
      .join(" ")
      .toLowerCase();
    if (needle && !text.includes(needle)) return false;
    if (availability === "all") return true;
    return tileAvailability(tile) === availability;
  });
}

export const MAX_QUANTITY = 1000;
/** What is wrong with a typed quantity, or null when it can be quoted. */
export function quantityProblem(text: string): string | null {
  const trimmed = text.trim();
  if (!/^\d{1,4}$/.test(trimmed)) return "Enter a whole number from 1 to 1000.";
  const value = Number(trimmed);
  return value < 1 || value > MAX_QUANTITY
    ? "Enter a whole number from 1 to 1000."
    : null;
}

/** "Bought 1 Foundations pack." */
export const boughtMessage = (setName: string, quantity: number): string =>
  `Bought ${quantity} ${setName} ${quantity === 1 ? "pack" : "packs"}.`;

export type PurchaseStage =
  | { kind: "pending" }
  /** Sent, and no answer came. Only Check status may settle it. */
  | { kind: "unconfirmed"; isChecking: boolean }
  | { kind: "rejected"; message: string };
export type Purchase = {
  tileId: string;
  quantity: number;
  terms: QuoteTerms;
  stage: PurchaseStage;
};

export type BallotCandidate = {
  setId: string;
  name: string;
  code: string | null;
};
export type BallotResult = { name: string; votes: number; packs: number };
export type BallotSave =
  | { kind: "pending" }
  | { kind: "unconfirmed"; isChecking: boolean }
  | { kind: "rejected"; message: string };
/**
 * The vote for the next supply drop. "loading" holds placeholders; "none"
 * means there is no current vote; "unavailable" means the account cannot
 * vote here at all.
 */
export type Ballot =
  | { status: "loading" }
  | { status: "none" }
  | { status: "unavailable" }
  | {
      status: "ready";
      /** "Dec 25". */
      closes: string;
      /** "Jan 1". */
      nextDrop: string;
      isOpen: boolean;
      /** Open, and this account has played a qualifying game. */
      canVote: boolean;
      qualifies: boolean;
      candidates: readonly BallotCandidate[];
      /** The saved choices, by set. */
      saved: readonly string[];
      /** Winners once published; null until then. */
      results: readonly BallotResult[] | null;
      /** The last read failed; the shown choices are the last good ones. */
      readFailed: boolean;
      save: BallotSave | null;
    };
export const BALLOT_LIMIT = 10;

export type Completed =
  { kind: "bought"; setName: string; quantity: number } | { kind: "voted" };

export type SupplyGap =
  "loading" | "signed-out" | "unregistered" | "unavailable";
export type SupplySource =
  | { status: SupplyGap }
  | {
      status: "ready";
      tiles: readonly SupplyTile[];
      isAppending: boolean;
      /** True when the first read failed, so the gallery is not known to be empty. */
      isUnread: boolean;
      failure: string | null;
      dismissFailure: () => void;
      balance: number | null;
      canBuy: boolean;
      /** The quote for the open purchase dialog's product and quantity. */
      quote: {
        tileId: string;
        quantity: number;
        state: "loading" | "ready" | "failed";
        terms: QuoteTerms | null;
      } | null;
      /** Says which product and quantity to quote; null stops quoting. */
      quoteFor: (tileId: string | null, quantity: number) => void;
      purchase: Purchase | null;
      buy: (tileId: string, quantity: number, terms: QuoteTerms) => void;
      checkPurchase: () => void;
      dismissPurchase: () => void;
      /** Null where there is no ballot source at all; the banner is then not shown. */
      ballot: Ballot | null;
      saveBallot: (setIds: readonly string[]) => void;
      checkBallot: () => void;
      dismissBallot: () => void;
      completed: Completed | null;
      dismissCompleted: () => void;
    };
export type ReadySupply = Extract<SupplySource, { status: "ready" }>;

/** The UTC quarter's ballot dates: it closes 168 hours before the next quarter starts. */
export function ballotDates(now: Date): { closes: Date; nextDrop: Date } {
  const year = now.getUTCFullYear();
  const quarter = Math.floor(now.getUTCMonth() / 3);
  const nextDrop = new Date(Date.UTC(year, (quarter + 1) * 3, 1));
  return { closes: new Date(nextDrop.getTime() - 168 * 3600 * 1000), nextDrop };
}
/** "Dec 25", in UTC. */
export const shortDate = (date: Date): string =>
  date.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  });
