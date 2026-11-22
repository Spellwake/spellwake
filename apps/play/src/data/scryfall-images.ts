/**
 * Official card images from Scryfall, built from the Scryfall card ID the
 * catalogue stores for every printing (`externalPrintingRef`). The server
 * builds the same links (`app.scryfall_image_url`); keep the two in step.
 */
const CARD_ID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** "normal" is the printed card; "art_crop" is its artwork alone. */
export type ScryfallImage = "normal" | "art_crop";

/** The image's address, or null for a reference that is not a Scryfall card ID. */
export function scryfallImage(
  externalPrintingRef: string,
  version: ScryfallImage,
): string | null {
  if (!CARD_ID.test(externalPrintingRef)) return null;
  const [first = "", second = ""] = externalPrintingRef;
  return `https://cards.scryfall.io/${version}/front/${first}/${second}/${externalPrintingRef}.jpg`;
}
