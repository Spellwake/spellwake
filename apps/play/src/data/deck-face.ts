/**
 * Which card a deck box shows. Every deck with cards has one: the face card
 * its owner chose while that card is still in the deck, otherwise the first
 * card added. `cards` are the deck's card names in the order they were
 * added, already narrowed to those with artwork to show.
 */
export function faceCardOf(
  chosen: string | null,
  cards: readonly string[],
): string | null {
  if (chosen !== null && cards.includes(chosen)) return chosen;
  return cards[0] ?? null;
}
