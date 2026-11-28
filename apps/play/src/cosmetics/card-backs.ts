/** Built-in Spellwake artwork and the standard Magic back served by Scryfall. */
export const SPELLWAKE_CARD_BACK_URL = `${import.meta.env.BASE_URL}cards/spellwake-sleeve-midnight.svg`;
export const MAGIC_CARD_BACK_URL =
  "https://backs.scryfall.io/normal/0/a/0aeebaf5-8c7d-4636-9e82-8c27447861f7.jpg";

/** Matches the free catalogue entry installed by the Magic card back migration. */
export const MAGIC_CARD_BACK_ID = "ffffffff-ffff-4fff-bfff-ffffffffffff";
export const PREVIEW_MAGIC_CARD_BACK_ID = "preview-back-magic";

/** Resolve the viewer's chosen built-in back, with Spellwake as the default. */
export function cardBackUrl(backId: string | null | undefined): string {
  return backId === MAGIC_CARD_BACK_ID || backId === PREVIEW_MAGIC_CARD_BACK_ID
    ? MAGIC_CARD_BACK_URL
    : SPELLWAKE_CARD_BACK_URL;
}
