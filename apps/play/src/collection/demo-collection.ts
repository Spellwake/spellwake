import { useCallback } from "react";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { z } from "zod";

/**
 * What the local preview person owns: cards by name and finish, and sealed
 * packs, and coins. Cards, Packs and Trade share it, so a pack opened in the
 * preview adds its cards to the preview collection. Nothing here reaches an account.
 */
const holdingSchema = z.strictObject({
  nonfoil: z.number().int().nonnegative(),
  foil: z.number().int().nonnegative(),
});
const packSchema = z.strictObject({
  id: z.string().min(1),
  /** The product's name, such as "Foundations Play Booster". */
  product: z.string().min(1),
  setName: z.string().min(1),
  setCode: z.string().min(1),
  owned: z.number().int().nonnegative(),
  /** Copies reserved in a trade: still owned, but not available to open. */
  held: z.number().int().nonnegative(),
});
const collectionSchema = z.strictObject({
  version: z.literal(1),
  cards: z.record(z.string(), holdingSchema),
  packs: z.array(packSchema),
  /** The preview wallet. Collections saved before it existed start at the seed. */
  coins: z.number().int().nonnegative().optional(),
});
export type DemoHolding = z.infer<typeof holdingSchema>;
export type DemoPack = z.infer<typeof packSchema>;
export type DemoCollection = z.infer<typeof collectionSchema>;

const holding = (nonfoil: number, foil = 0): DemoHolding => ({ nonfoil, foil });
const pack = (
  setName: string,
  setCode: string,
  owned: number,
  held = 0,
): DemoPack => ({
  id: `preview-pack-${setCode.toLowerCase()}`,
  product: `${setName} Play Booster`,
  setName,
  setCode,
  owned,
  held,
});
/** The preview wallet's starting balance, as in the approved Market walkthroughs. */
export const DEMO_COINS = 8000;
export const coinsOf = (collection: DemoCollection): number =>
  collection.coins ?? DEMO_COINS;
/** The preview's starting collection: every card of the preview catalogue, and four products of packs. */
export const DEMO_COLLECTION_SEED: DemoCollection = {
  version: 1,
  cards: {
    "Skirk Prospector": holding(4),
    "Mogg War Marshal": holding(4, 1),
    "Goblin Instigator": holding(3),
    "Goblin Warchief": holding(4),
    "Goblin Chieftain": holding(2),
    "Goblin Matron": holding(3, 1),
    "Lightning Bolt": holding(4, 3),
    Shock: holding(6),
    "Krenko's Command": holding(4),
    Mountain: holding(20),
    Abrade: holding(2),
    "Lightning Strike": holding(3),
    "Krenko, Mob Boss": holding(1, 1),
    "Aether Vial": holding(1),
    "Conspicuous Snoop": holding(2),
    "Goblin Bushwhacker": holding(4),
    "Goblin Guide": holding(4, 2),
    "Goblin Piledriver": holding(1),
    "Legion Loyalist": holding(2),
    "Word of Seizing": holding(1),
  },
  packs: [
    pack("Foundations", "FDN", 4),
    pack("Modern Horizons 3", "MH3", 2),
    pack("Bloomburrow", "BLB", 3, 1),
    pack("Duskmourn", "DSK", 1),
  ],
  coins: DEMO_COINS,
};

/** Where the preview collection was kept before it had its own key. */
const EARLIER_KEY = "spellwake:collection:demo:v1";
const storage = {
  getItem: (key: string, initial: DemoCollection): DemoCollection => {
    try {
      let saved = window.localStorage.getItem(key);
      if (saved === null) {
        // A collection saved under the key it once shared moves to its own, once.
        const earlier = window.localStorage.getItem(EARLIER_KEY);
        if (
          earlier !== null &&
          collectionSchema.safeParse(JSON.parse(earlier)).success
        ) {
          window.localStorage.setItem(key, earlier);
          window.localStorage.removeItem(EARLIER_KEY);
          saved = earlier;
        }
      }
      if (saved === null) return initial;
      const parsed = collectionSchema.safeParse(JSON.parse(saved));
      return parsed.success ? parsed.data : initial;
    } catch {
      return initial;
    }
  },
  setItem: (key: string, value: DemoCollection): void => {
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
/** Its own key: the older preview deck library still reads `spellwake:collection:demo:v1`. */
export const DEMO_COLLECTION_KEY = "spellwake:holdings:demo:v1";
const collectionAtom = atomWithStorage<DemoCollection>(
  DEMO_COLLECTION_KEY,
  DEMO_COLLECTION_SEED,
  storage,
  { getOnInit: true },
);

export function useDemoCollection(): [
  DemoCollection,
  (update: (current: DemoCollection) => DemoCollection) => void,
] {
  const [collection, setCollection] = useAtom(collectionAtom);
  const update = useCallback(
    (change: (current: DemoCollection) => DemoCollection) =>
      setCollection((current) => change(current)),
    [setCollection],
  );
  return [collection, update];
}
