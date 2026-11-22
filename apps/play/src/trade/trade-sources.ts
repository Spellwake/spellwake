import type { Portrait } from "../account/account-sources";

/**
 * One shared trade room between you and one other person, as the approved
 * desktop design describes it: each side edits only its own offer, Ready
 * validates and reserves that side, both confirm the exact current terms,
 * and any edit resets both. The screen reads it from the local preview or
 * from the account.
 */

export type TradeItemKind = "card" | "pack" | "coin";

/** Something that can be offered: one printing and finish, one pack product, or coins. */
export type TradeAsset = {
  id: string;
  kind: TradeItemKind;
  name: string;
  /** "M11 · Instant", or null. Packs and coins have none. */
  detail: string | null;
  /** A card's artwork, cropped from its face, or null. */
  artUrl: string | null;
  /** A pack's set, which colours its wrapper. */
  setName: string | null;
};
/** An offered quantity of one asset. */
export type OfferLine = TradeAsset & { quantity: number };
/** What you may offer of one asset: what you own less what other trades hold. */
export type TradeHolding = TradeAsset & { owned: number };

/** "editing" is Not ready. */
export type Readiness = "editing" | "ready" | "confirmed";

export type TradeStage =
  | { kind: "open" }
  /** The final Trade was sent and no answer came. Only Check status may settle it. */
  | { kind: "unconfirmed"; isChecking: boolean }
  | {
      kind: "complete";
      gave: readonly OfferLine[];
      received: readonly OfferLine[];
    }
  | { kind: "closed"; reason: "canceled" | "unavailable" };

/** The command in flight, which disables a second press. */
export type TradePending = "ready" | "edit" | "trade" | "cancel" | null;

export type TradeParty = {
  status: Readiness;
  lines: readonly OfferLine[];
};

export type TradeGap =
  "loading" | "signed-out" | "unregistered" | "unavailable";
export type TradeSource =
  | { status: TradeGap }
  | {
      status: "ready";
      partner: { id: string; name: string; portrait: Portrait | null };
      own: TradeParty;
      other: TradeParty;
      /** Your own holdings, for Add items and the quantity limits. */
      holdings: readonly TradeHolding[];
      holdingsState: "loading" | "ready" | "failed";
      stage: TradeStage;
      pending: TradePending;
      /** A passive message to say once, such as a known rejection. */
      notice: string | null;
      dismissNotice: () => void;
      /** Sets one offered quantity. Zero removes the line. */
      setQuantity: (assetId: string, quantity: number) => void;
      /** Adds one more of an asset, as Add items' plus does. */
      add: (assetId: string) => void;
      ready: () => void;
      /** Returns a ready offer to editing; both sides become Not ready. */
      editItems: () => void;
      /** Confirms the exact current terms. */
      trade: () => void;
      /** Closes the trade for both people. */
      cancel: () => void;
      /** Asks again about an unconfirmed Trade, never sending a new one. */
      checkStatus: () => void;
    };
export type ReadyTrade = Extract<TradeSource, { status: "ready" }>;

/** Where a trade room lives. `from` is the surface Back and Done return to. */
export const tradePath = (personId: string): string =>
  `/collection/trade/${encodeURIComponent(personId)}`;

export const COINS_ID = "coins";

/** A quantity limited to a whole number between zero and what is owned. */
export const capQuantity = (quantity: number, owned: number): number =>
  Math.max(0, Math.min(Math.max(0, owned), Math.floor(quantity)));

/** Lines that ask for more than is owned, with the concise reason to show. */
export function shortfalls(
  lines: readonly OfferLine[],
  holdings: readonly TradeHolding[],
): { id: string; reason: string }[] {
  return lines.flatMap((line) => {
    const owned = holdings.find((held) => held.id === line.id)?.owned ?? 0;
    if (line.quantity <= owned) return [];
    return [
      {
        id: line.id,
        reason:
          line.kind === "coin"
            ? `${owned.toLocaleString("en-US")} coins available.`
            : owned === 0
              ? `${line.name} is no longer available.`
              : `Only ${owned} ${line.name} available.`,
      },
    ];
  });
}

/** Ready needs something offered on either side and every own line covered. */
export function canMarkReady(
  own: readonly OfferLine[],
  other: readonly OfferLine[],
  holdings: readonly TradeHolding[],
): boolean {
  const offers = (lines: readonly OfferLine[]) =>
    lines.some((line) => line.quantity > 0);
  return (
    (offers(own) || offers(other)) && shortfalls(own, holdings).length === 0
  );
}

/** Sets one line, adding or removing it, keeping the order lines were added in. */
export function withQuantity(
  lines: readonly OfferLine[],
  asset: TradeAsset,
  quantity: number,
): OfferLine[] {
  const exists = lines.some((line) => line.id === asset.id);
  const next = exists
    ? lines.map((line) => (line.id === asset.id ? { ...line, quantity } : line))
    : [...lines, { ...asset, quantity }];
  return next.filter((line) => line.quantity > 0);
}

/** Coins last, as the approved trays order them. */
export const inTrayOrder = (lines: readonly OfferLine[]): OfferLine[] => [
  ...lines.filter((line) => line.kind !== "coin"),
  ...lines.filter((line) => line.kind === "coin"),
];

/** The status line's words, which name the other person as the design does. */
export function statusWords(
  own: Readiness,
  other: Readiness,
  partner: string,
): string | null {
  if (own === "editing") return null;
  if (own === "confirmed") return `Waiting for ${partner} to confirm`;
  return other === "editing" ? `Waiting for ${partner}` : "Both ready";
}
