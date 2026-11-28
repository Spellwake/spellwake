import type { Outcome, SourceGap } from "../decks/deck-sources";
import type { PlaymatEffectId } from "spellwake-renderer-benchmark/playmat-effects";
import type { WearTierId } from "spellwake-renderer-benchmark/wear-effects";

/**
 * What Default settings and Cosmetics ask of their data. Each screen takes one
 * of these and nothing else, so it runs the same on the signed-in account and
 * on the local preview. A source says plainly what it does not know: an item
 * with no artwork has `look: null`, and one with no listed price has
 * `price: null`. Neither is ever filled in with a guess.
 */

/** Pets are in the approved design. A source lists them only if it has them. */
export type CosmeticKind =
  | "sleeve"
  | "back"
  | "dice"
  | "playmat"
  | "card_effect"
  | "sleeve_effect"
  | "playmat_effect"
  | "pet";
export const COSMETIC_KINDS: readonly CosmeticKind[] = [
  "sleeve",
  "back",
  "dice",
  "playmat",
  "card_effect",
  "sleeve_effect",
  "playmat_effect",
  "pet",
];

/** How an item is drawn: artwork, a playmat surface, a procedural wear effect, or a pattern painted in code. */
export type CosmeticLook =
  | { imageUrl: string }
  | { paint: string }
  | { playmat: { id: string } }
  | { playmatEffect: { id: PlaymatEffectId } }
  | { wear: { target: "card" | "sleeve"; tier: WearTierId } };

export type CosmeticItem = {
  id: string;
  name: string;
  kind: CosmeticKind;
  look: CosmeticLook | null;
};

/** One choice per category. Null is "None", which only some categories allow. */
export type DefaultSelection = Readonly<Record<CosmeticKind, string | null>>;

/** What an appearance may be chosen from: the categories, and the items the account owns. */
export type AppearanceOptions = {
  categories: readonly CosmeticKind[];
  /** Categories that may be left at None. */
  optional: readonly CosmeticKind[];
  /** "failed" is a list that could not be read. */
  items: {
    state: "loading" | "ready" | "failed";
    items: readonly CosmeticItem[];
  };
};

export type DefaultSettingsSource =
  | { status: SourceGap }
  | {
      status: "ready";
      categories: readonly CosmeticKind[];
      /** Categories that may be left at None. */
      optional: readonly CosmeticKind[];
      /** What the account owns. "failed" is a list that could not be read. */
      options: {
        state: "loading" | "ready" | "failed";
        items: readonly CosmeticItem[];
      };
      /** The defaults as last saved. New decks start from these. */
      saved: DefaultSelection;
      /** `message` on failure is ready to show: it tells a failed save from an unconfirmed one. */
      save: (selection: DefaultSelection) => Promise<Outcome>;
    };

export type ShopItem = CosmeticItem & {
  isOwned: boolean;
  /** True for an item sold by itself for cash. */
  isSold: boolean;
  /** The listed price, such as "$4.99", or null where the source has none to show. */
  price: string | null;
};

/** One line of passive status about the chosen item's purchase. */
export type CheckoutNotice = {
  text: string;
  /** A working line stays until replaced. A result leaves by itself. */
  kind: "working" | "result";
};

export type CosmeticsSource =
  | { status: SourceGap }
  | {
      status: "ready";
      categories: readonly CosmeticKind[];
      /** "failed" keeps whatever was loaded. It is never shown as an empty shop. */
      state: "loading" | "ready" | "failed";
      items: readonly ShopItem[];
      /**
       * Buying one item at a time through hosted checkout. Null where nothing
       * can be bought. The source follows the item named by `choose`.
       */
      checkout: {
        choose: (itemId: string | null) => void;
        isBusy: boolean;
        notice: CheckoutNotice | null;
        /** Opens hosted checkout for the chosen item. It never grants ownership itself. */
        buy: () => void;
      } | null;
    };

export const KIND_LABEL: Readonly<Record<CosmeticKind, string>> = {
  sleeve: "Sleeves",
  back: "Card backs",
  dice: "Dice",
  playmat: "Playmats",
  card_effect: "Card effects",
  sleeve_effect: "Sleeve effects",
  playmat_effect: "Playmat effects",
  pet: "Pets",
};
/** How one item of a kind is described under its name. */
export const KIND_NOUN: Readonly<Record<CosmeticKind, string>> = {
  sleeve: "Card sleeve",
  back: "Card back",
  dice: "Dice",
  playmat: "Playmat",
  card_effect: "Card effect",
  sleeve_effect: "Sleeve effect",
  playmat_effect: "Playmat effect",
  pet: "Pet",
};

/** The first item of a kind, or None where the kind allows it. Used when a saved choice is gone. */
export function firstChoice(
  items: readonly CosmeticItem[],
  kind: CosmeticKind,
): string | null {
  return items.find((item) => item.kind === kind)?.id ?? null;
}

/** Items of a kind whose names contain the search, in the order the source gave them. */
export function matching<Item extends CosmeticItem>(
  items: readonly Item[],
  kind: CosmeticKind,
  query: string,
): Item[] {
  const needle = query.trim().toLowerCase();
  return items.filter(
    (item) =>
      item.kind === kind &&
      (needle === "" || item.name.toLowerCase().includes(needle)),
  );
}

/** True when two selections choose the same item in every listed category. */
export function sameSelection(
  a: DefaultSelection,
  b: DefaultSelection,
  categories: readonly CosmeticKind[],
): boolean {
  return categories.every((kind) => a[kind] === b[kind]);
}
