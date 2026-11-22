/**
 * Deck lists that arrive as files or links, turned into the plain text the
 * paste box takes. Every source then goes through the same reading, so a
 * file is read exactly as the same list pasted would be.
 */

/** The largest deck file read. Larger is refused before it is opened. */
export const MAX_DECK_FILE_BYTES = 1_000_000;
export const DECK_FILE_TYPES = [".txt", ".dek", ".cod"] as const;

const ENTITY: Readonly<Record<string, string>> = {
  amp: "&",
  lt: "<",
  gt: ">",
  quot: '"',
  apos: "'",
};
/** XML text with its character references decoded. Unknown references are kept as written. */
function decodeXml(value: string): string {
  return value.replace(
    /&(#x[0-9a-f]+|#[0-9]+|[a-z]+);/gi,
    (whole, reference: string) => {
      if (reference.startsWith("#x") || reference.startsWith("#X")) {
        const code = Number.parseInt(reference.slice(2), 16);
        return Number.isFinite(code) && code <= 0x10ffff
          ? String.fromCodePoint(code)
          : whole;
      }
      if (reference.startsWith("#")) {
        const code = Number.parseInt(reference.slice(1), 10);
        return Number.isFinite(code) && code <= 0x10ffff
          ? String.fromCodePoint(code)
          : whole;
      }
      return ENTITY[reference.toLowerCase()] ?? whole;
    },
  );
}
/** The attributes of one XML start tag, by lower-case name. */
function attributes(tag: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const match of tag.matchAll(
    /([A-Za-z_][\w.-]*)\s*=\s*("([^"]*)"|'([^']*)')/g,
  ))
    found.set(
      (match[1] ?? "").toLowerCase(),
      decodeXml(match[3] ?? match[4] ?? ""),
    );
  return found;
}
const tags = (text: string, name: string): string[] =>
  Array.from(text.matchAll(new RegExp(`<${name}\\b[^>]*>`, "gi")), (m) => m[0]);

/** A Magic Online deck: one `Cards` element per card, with its quantity and sideboard flag. */
export function textFromMtgoDeck(xml: string): string | null {
  if (!/<Deck\b/i.test(xml)) return null;
  const main: string[] = [];
  const side: string[] = [];
  for (const tag of tags(xml, "Cards")) {
    const values = attributes(tag);
    const name = values.get("name")?.trim();
    const quantity = values.get("quantity")?.trim();
    if (!name || !quantity || !/^\d+$/.test(quantity)) continue;
    (values.get("sideboard")?.toLowerCase() === "true" ? side : main).push(
      `${quantity} ${name}`,
    );
  }
  return [...main, ...(side.length ? ["", "Sideboard", ...side] : [])].join(
    "\n",
  );
}

/** A Cockatrice deck: `card` elements inside a main or side `zone`. */
export function textFromCockatriceDeck(xml: string): string | null {
  if (!/<cockatrice_deck\b/i.test(xml)) return null;
  const lines: string[] = [];
  for (const zone of xml.matchAll(/<zone\b([^>]*)>([\s\S]*?)<\/zone>/gi)) {
    const zoneName = attributes(`<zone ${zone[1] ?? ""}>`).get("name");
    const cards = tags(zone[2] ?? "", "card").flatMap((tag) => {
      const values = attributes(tag);
      const name = values.get("name")?.trim();
      const quantity = values.get("number")?.trim();
      return name && quantity && /^\d+$/.test(quantity)
        ? [`${quantity} ${name}`]
        : [];
    });
    if (!cards.length) continue;
    // Tokens are not part of a deck.
    if (zoneName === "tokens") continue;
    if (zoneName === "side") lines.push("", "Sideboard");
    else if (lines.length) lines.push("", "Deck");
    lines.push(...cards);
  }
  return lines.join("\n").trim();
}

export type DeckFileReading =
  | { status: "read"; text: string }
  /** Not a .txt, .dek or .cod file, or one that is not what its name says. */
  | { status: "unsupported" };

/** The list a deck file holds, as the paste box would take it. */
export function deckTextFromFile(
  fileName: string,
  contents: string,
): DeckFileReading {
  const extension = /\.[^.]+$/.exec(fileName.toLowerCase())?.[0];
  const text =
    extension === ".txt"
      ? contents
      : extension === ".dek"
        ? textFromMtgoDeck(contents)
        : extension === ".cod"
          ? textFromCockatriceDeck(contents)
          : null;
  return text === null ? { status: "unsupported" } : { status: "read", text };
}

/**
 * Whether an address is a public Moxfield or TappedOut deck. Anything else is
 * refused before anything is fetched.
 */
export function isDeckLink(value: string): boolean {
  try {
    const url = new URL(value.trim());
    if (url.protocol !== "https:" || url.username || url.password) return false;
    const host = url.hostname.replace(/^www\./, "");
    const path = url.pathname.split("/").filter(Boolean);
    return (
      (host === "moxfield.com" && path[0] === "decks" && path.length >= 2) ||
      (host === "tappedout.net" && path[0] === "mtg-decks" && path.length >= 2)
    );
  } catch {
    return false;
  }
}
