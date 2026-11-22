import { useMemo } from "react";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { z } from "zod";

/**
 * The preview market's own state: example listings bought, and your own
 * listings with the cards or packs they hold. Cards, Packs and Trade read the
 * holds so a listed copy is never counted as usable elsewhere.
 */
const mineSchema = z.strictObject({
  id: z.string().min(1),
  assetId: z.string().min(1),
  quantity: z.number().int().positive(),
  total: z.number().int().positive(),
  listedAt: z.number(),
});
const marketSchema = z.strictObject({
  version: z.literal(1),
  /** Example listings bought in this preview, which leave the market. */
  sold: z.array(z.string()),
  mine: z.array(mineSchema),
});
type DemoMarket = z.infer<typeof marketSchema>;
export type MyListing = z.infer<typeof mineSchema>;
const EMPTY: DemoMarket = { version: 1, sold: [], mine: [] };

const storage = {
  getItem: (key: string, initial: DemoMarket): DemoMarket => {
    try {
      const saved = window.localStorage.getItem(key);
      if (saved === null) return initial;
      const parsed = marketSchema.safeParse(JSON.parse(saved));
      return parsed.success ? parsed.data : initial;
    } catch {
      return initial;
    }
  },
  setItem: (key: string, value: DemoMarket): void => {
    try {
      window.localStorage.setItem(key, JSON.stringify(value));
    } catch {
      // A private window may refuse storage. The preview then forgets on reload.
    }
  },
  removeItem: (key: string): void => {
    try {
      window.localStorage.removeItem(key);
    } catch {
      // As above.
    }
  },
};
export const DEMO_MARKET_KEY = "spellwake:market:demo:v1";
export const marketAtom = atomWithStorage<DemoMarket>(
  DEMO_MARKET_KEY,
  EMPTY,
  storage,
  {
    getOnInit: true,
  },
);

/** A card asset's identity in the preview: its name and finish. */
export const cardAssetId = (name: string, foil: boolean): string =>
  `card:${name}:${foil ? "foil" : "nonfoil"}`;

/** Units your own listings hold, by asset. Trade and Packs leave them out of what is available. */
export function useDemoListingHolds(): Readonly<Record<string, number>> {
  const [market] = useAtom(marketAtom);
  return useMemo(() => {
    const holds: Record<string, number> = {};
    for (const mine of market.mine)
      holds[mine.assetId] = (holds[mine.assetId] ?? 0) + mine.quantity;
    return holds;
  }, [market.mine]);
}
