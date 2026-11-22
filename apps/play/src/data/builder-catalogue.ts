import { useQuery } from "@tanstack/react-query";
import { z } from "zod";

/**
 * Cards the deck builder can search. This is a local preview set prepared from
 * the review fixtures. It stands in for the catalogue service, which is not
 * connected to this screen yet, and carries only what its source records say:
 * a card with no legality record is reported as unknown, never assumed legal.
 */
const fixtureCardSchema = z.object({
  name: z.string().min(1),
  image: z.string().min(1),
  art_image: z.string().min(1),
  mana: z.string(),
  mana_value: z.number().nonnegative(),
  type: z.string().min(1),
  rules: z.string().optional(),
  artist: z.string().optional(),
  colors: z.array(z.string()).optional(),
  color_identity: z.array(z.string()).optional(),
  legalities: z.record(z.string(), z.string()).optional(),
  /** The printing's catalogue page, which names its set and collector number. */
  source: z.string().optional(),
});

export type BuilderCard = {
  name: string;
  faceUrl: string;
  artUrl: string;
  /** Mana symbols in printed order, such as ["1", "R"]. */
  cost: readonly string[];
  manaValue: number;
  type: string;
  rules: string;
  /** Who painted this printing's artwork, or null when the source does not say. */
  artist: string | null;
  colors: readonly string[];
  /** Legality by lower-case format name, or null when the source has no record. */
  legality: Readonly<Record<string, string>> | null;
  /** Set code and collector number of this printing, or null when the source does not say. */
  printing: { set: string; number: string } | null;
};
export type FormatLegality = "legal" | "not-legal" | "unknown";

const root = `${import.meta.env.BASE_URL}builder-preview/`;
/** Card names are matched without regard to case or apostrophe style. */
export const cardKey = (name: string): string =>
  name.trim().replaceAll("’", "'").toLowerCase();
export const manaSymbolUrl = (symbol: string): string =>
  `${root}mana/${symbol.replaceAll("/", "-")}.svg`;

/** Reads the set and collector number out of a printing's catalogue address. */
export function printingFrom(
  source: string | undefined,
): BuilderCard["printing"] {
  const match = /\/card\/([a-z0-9]+)\/([^/?#]+)/i.exec(source ?? "");
  return match?.[1] && match[2]
    ? { set: match[1].toUpperCase(), number: decodeURIComponent(match[2]) }
    : null;
}

async function loadCatalogue(signal: AbortSignal): Promise<BuilderCard[]> {
  const response = await fetch(`${root}catalog.json`, { signal });
  if (!response.ok) throw new Error(`Catalogue responded ${response.status}.`);
  return z
    .array(fixtureCardSchema)
    .parse(await response.json())
    .map((card) => ({
      name: card.name,
      faceUrl: root + card.image,
      artUrl: root + card.art_image,
      cost: Array.from(
        card.mana.matchAll(/\{([^}]+)\}/g),
        (match) => match[1] ?? "",
      ),
      manaValue: card.mana_value,
      type: card.type,
      rules: card.rules ?? "",
      artist: card.artist ?? null,
      colors: card.colors ?? card.color_identity ?? [],
      legality: card.legalities ?? null,
      printing: printingFrom(card.source),
    }));
}

/** Whether a card may be added to a deck of this format, as far as its record says. */
export function legalityIn(
  card: BuilderCard | undefined,
  format: string,
): FormatLegality {
  const record = card?.legality?.[format.toLowerCase()];
  if (record === undefined) return "unknown";
  return record === "legal" || record === "restricted" ? "legal" : "not-legal";
}

export type BuilderCatalogue = {
  cards: readonly BuilderCard[];
  find: (name: string) => BuilderCard | undefined;
};

export function useBuilderCatalogue(): {
  catalogue: BuilderCatalogue | null;
  hasFailed: boolean;
} {
  const query = useQuery({
    queryKey: ["builder-catalogue"],
    queryFn: ({ signal }) => loadCatalogue(signal),
    staleTime: Infinity,
    select: (cards): BuilderCatalogue => {
      const byName = new Map(cards.map((card) => [cardKey(card.name), card]));
      return { cards, find: (name) => byName.get(cardKey(name)) };
    },
  });
  return { catalogue: query.data ?? null, hasFailed: query.isError };
}
