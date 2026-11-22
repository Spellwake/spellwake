import { MANA_ORDER, type ManaColor } from "./appearance";

export { MANA_ORDER, type ManaColor };
/**
 * How a deck box is drawn: its colour identity sets the leather, and its
 * face card, chosen from the deck, is printed on the flap from the card's
 * official artwork.
 */
export type BoxAppearance = {
  colors: readonly ManaColor[];
  /** The face card's official art crop (Scryfall), or null for plain leather. */
  faceArt: string | null;
};
export const DEFAULT_APPEARANCE: BoxAppearance = { colors: [], faceArt: null };
