import { useQuery } from "@tanstack/react-query";
import { z } from "zod";
import { packStill } from "@spellwake/booster-pack";
import { packPrint } from "./pack-palette";

/** The Spellwake card back, drawn in SVG, for cards dealt face down. */
export const CARD_BACK = `${import.meta.env.BASE_URL}cards/spellwake-sleeve-midnight.svg`;

/**
 * The official artwork a wrapper may carry, and the 3D stills of sealed
 * packs. Set symbols are Scryfall's copies of the official symbols,
 * prepared into `sets/` with their provenance; nothing is drawn in place
 * of a symbol that is missing.
 */
const SETS = `${import.meta.env.BASE_URL}sets/`;
const indexSchema = z.array(
  z.object({ code: z.string(), name: z.string(), file: z.string() }),
);

/** Where a set's symbol is known: by its code, else by its name. */
export type SetSymbol = {
  /** False until the symbol index has been read or has failed. */
  isSettled: boolean;
  url: string | null;
};

export function useSetSymbol(
  setCode: string | null,
  setName: string,
): SetSymbol {
  const index = useQuery({
    queryKey: ["set-symbols"],
    queryFn: async () => {
      const response = await fetch(`${SETS}index.json`);
      if (!response.ok) throw new Error("No set symbols are available.");
      return indexSchema.parse(await response.json());
    },
    staleTime: Infinity,
    gcTime: Infinity,
  });
  const code = setCode?.toLowerCase();
  const name = setName.toLowerCase();
  const entry = index.data?.find(
    (set) =>
      set.code === code ||
      set.name.toLowerCase() === name ||
      // Scryfall's full names carry subtitles: "Duskmourn: House of Horror".
      set.name.toLowerCase().startsWith(`${name}:`),
  );
  const remoteUrl =
    !entry && code && /^[a-z0-9]{2,6}$/.test(code)
      ? `https://svgs.scryfall.io/sets/${code}.svg`
      : null;
  return {
    isSettled: !index.isPending,
    url: entry ? `${SETS}${entry.file}` : remoteUrl,
  };
}

/** Pixels across a still: the library's widest wrapper at twice its size. */
const STILL_WIDTH = 480;

/**
 * A still of the sealed pack, rendered from the same 3D model as the
 * opening, as pixels for a canvas (never an image file). Null while it
 * renders or where 3D is unavailable, when the drawn wrapper stands in.
 * Stills are kept for the session.
 */
export function usePackStill(
  setName: string,
  product: string,
  symbol: SetSymbol,
): ImageBitmap | null {
  const still = useQuery({
    queryKey: ["pack-still", setName, product, symbol.url, STILL_WIDTH],
    queryFn: () =>
      packStill(packPrint(setName, product, symbol.url), STILL_WIDTH).catch(
        (error: unknown) => {
          console.warn(
            "Showing the drawn wrapper instead of the 3D still.",
            error,
          );
          throw error;
        },
      ),
    enabled: symbol.isSettled,
    staleTime: Infinity,
    gcTime: Infinity,
  });
  return still.data ?? null;
}
