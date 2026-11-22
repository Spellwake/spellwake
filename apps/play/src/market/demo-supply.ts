import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { z } from "zod";
import {
  coinsOf,
  useDemoCollection,
  type DemoCollection,
} from "../collection/demo-collection";
import {
  ballotDates,
  shortDate,
  type Ballot,
  type BallotCandidate,
  type BallotSave,
  type Completed,
  type Purchase,
  type QuoteTerms,
  type SupplySource,
  type SupplyTile,
} from "./supply-sources";

/**
 * The local preview of Market Supply. Its products, stock and prices are
 * examples: each pack costs a little more than the one before, so a total is
 * summed pack by pack. Buying moves packs into the preview collection and
 * coins out of the preview wallet. The vote follows the real calendar: it
 * closes 168 hours before the next quarter. Nothing reaches an account.
 */

const QUOTE_MS = 350;
const COMMAND_MS = 800;

type Product = {
  id: string;
  setCode: string;
  setName: string;
  product: string;
  stock: number;
  base: number;
  step: number;
  /** The preview collection's pack this product adds to. */
  packId: string;
};
const product = (
  setCode: string,
  setName: string,
  kind: string,
  stock: number,
  base: number,
  step: number,
  packId = `preview-pack-${setCode.toLowerCase()}`,
): Product => ({
  id: `preview-recipe-${setCode.toLowerCase()}-${kind.toLowerCase().replaceAll(" ", "-")}`,
  setCode,
  setName,
  product: kind,
  stock,
  base,
  step,
  packId,
});
const PRODUCTS: readonly Product[] = [
  product("FDN", "Foundations", "Play Booster", 420, 1000, 3),
  product(
    "FDN",
    "Foundations",
    "Collector Booster",
    40,
    3000,
    25,
    "preview-pack-fdn-collector",
  ),
  product("MH3", "Modern Horizons 3", "Play Booster", 280, 1400, 4),
  product("BLB", "Bloomburrow", "Play Booster", 160, 1200, 3),
  product("DSK", "Duskmourn", "Play Booster", 0, 1300, 3),
];
/** Sets with no qualified product: shown by name, never with invented pack art. */
const BARE_SETS = [
  { code: "P15A", name: "15th Anniversary Cards" },
  { code: "PHTR", name: "2016 Heroes of the Realm" },
] as const;
/** The vote's frozen candidates. */
const CANDIDATES: readonly BallotCandidate[] = [
  ["FDN", "Foundations"],
  ["MH3", "Modern Horizons 3"],
  ["BLB", "Bloomburrow"],
  ["DSK", "Duskmourn"],
  ["DMR", "Dominaria Remastered"],
  ["ISD", "Innistrad"],
  ["KTK", "Khans of Tarkir"],
  ["KHM", "Kaldheim"],
  ["ELD", "Throne of Eldraine"],
  ["THB", "Theros Beyond Death"],
  ["BRO", "The Brothers’ War"],
].map(([code, name]) => ({
  setId: `preview-set-${code!.toLowerCase()}`,
  code: code!,
  name: name!,
}));

const supplySchema = z.strictObject({
  version: z.literal(1),
  /** Packs bought of each product in this preview. */
  sold: z.record(z.string(), z.number().int().nonnegative()),
  /** The saved vote, by set, or null before one is saved. */
  vote: z.array(z.string()).max(10).nullable(),
});
type DemoSupply = z.infer<typeof supplySchema>;
const EMPTY: DemoSupply = { version: 1, sold: {}, vote: null };
const storage = {
  getItem: (key: string, initial: DemoSupply): DemoSupply => {
    try {
      const saved = window.localStorage.getItem(key);
      if (saved === null) return initial;
      const parsed = supplySchema.safeParse(JSON.parse(saved));
      return parsed.success ? parsed.data : initial;
    } catch {
      return initial;
    }
  },
  setItem: (key: string, value: DemoSupply): void => {
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
export const DEMO_SUPPLY_KEY = "spellwake:supply:demo:v1";
const supplyAtom = atomWithStorage<DemoSupply>(
  DEMO_SUPPLY_KEY,
  EMPTY,
  storage,
  {
    getOnInit: true,
  },
);

/** The exact terms for a quantity: each pack costs `step` more than the one before. */
export function previewTerms(
  item: Product,
  sold: number,
  quantity: number,
): QuoteTerms {
  const remaining = Math.max(0, item.stock - sold);
  const total =
    quantity <= remaining
      ? quantity * item.base +
        item.step * (quantity * sold + (quantity * (quantity - 1)) / 2)
      : null;
  return {
    quantity,
    remaining,
    total,
    policyId: `${item.id}-policy`,
    revision: String(sold + 1),
    totalText: total === null ? null : String(total),
  };
}

/** Adds bought packs to the preview collection and takes their total from the wallet. */
function receive(
  collection: DemoCollection,
  item: Product,
  quantity: number,
  total: number,
): DemoCollection {
  const owned = collection.packs.some((pack) => pack.id === item.packId);
  return {
    ...collection,
    coins: coinsOf(collection) - total,
    packs: owned
      ? collection.packs.map((pack) =>
          pack.id === item.packId
            ? { ...pack, owned: pack.owned + quantity }
            : pack,
        )
      : [
          ...collection.packs,
          {
            id: item.packId,
            product: `${item.setName} ${item.product}`,
            setName: item.setName,
            setCode: item.setCode,
            owned: quantity,
            held: 0,
          },
        ],
  };
}

/** Preview Supply. `now` is the clock the vote's dates follow. */
export function useDemoSupply(now = new Date()): SupplySource {
  const [collection, updateCollection] = useDemoCollection();
  const [supply, setSupply] = useAtom(supplyAtom);
  const [quoteTarget, setQuoteTarget] = useState<{
    tileId: string;
    quantity: number;
  } | null>(null);
  const [quoteReady, setQuoteReady] = useState(false);
  const [purchase, setPurchase] = useState<Purchase | null>(null);
  const [save, setSave] = useState<BallotSave | null>(null);
  const [completed, setCompleted] = useState<Completed | null>(null);
  const timers = useRef<number[]>([]);
  useEffect(
    () => () => {
      for (const timer of timers.current) window.clearTimeout(timer);
    },
    [],
  );
  const later = useCallback((ms: number, run: () => void) => {
    timers.current.push(window.setTimeout(run, ms));
  }, []);

  // A new product or quantity is quoted after a moment, as a read would be.
  useEffect(() => {
    if (!quoteTarget) return;
    setQuoteReady(false);
    const timer = window.setTimeout(() => setQuoteReady(true), QUOTE_MS);
    return () => window.clearTimeout(timer);
  }, [quoteTarget]);

  const tiles = useMemo((): SupplyTile[] => {
    const products = PRODUCTS.map((item): SupplyTile => ({
      kind: "product",
      id: item.id,
      setId: `preview-set-${item.setCode.toLowerCase()}`,
      setName: item.setName,
      setCode: item.setCode,
      product: item.product,
      fullName: `${item.setName} ${item.product}`,
      quote: previewTerms(item, supply.sold[item.id] ?? 0, 1),
      quoteState: "ready",
    }));
    const bare = BARE_SETS.map((set): SupplyTile => ({
      kind: "set",
      id: `preview-set-${set.code.toLowerCase()}`,
      setName: set.name,
      setCode: set.code,
    }));
    return [...products, ...bare];
  }, [supply.sold]);

  const dates = ballotDates(now);
  const isOpen = now.getTime() < dates.closes.getTime();
  const ballot: Ballot = {
    status: "ready",
    closes: shortDate(dates.closes),
    nextDrop: shortDate(dates.nextDrop),
    isOpen,
    canVote: isOpen,
    qualifies: true,
    candidates: CANDIDATES,
    saved: supply.vote ?? [],
    results: null,
    readFailed: false,
    save,
  };

  // Stable, so a dialog can name its quote in an effect without re-quoting each render.
  const quoteFor = useCallback(
    (tileId: string | null, quantity: number) =>
      setQuoteTarget((current) =>
        tileId === null
          ? null
          : current?.tileId === tileId && current.quantity === quantity
            ? current
            : { tileId, quantity },
      ),
    [],
  );
  const productOf = (tileId: string) =>
    PRODUCTS.find((item) => item.id === tileId);
  const quoted = quoteTarget ? productOf(quoteTarget.tileId) : undefined;
  return {
    status: "ready",
    tiles,
    isAppending: false,
    isUnread: false,
    failure: null,
    dismissFailure: () => undefined,
    balance: coinsOf(collection),
    canBuy: true,
    quote:
      quoteTarget && quoted
        ? {
            ...quoteTarget,
            state: quoteReady ? "ready" : "loading",
            terms: quoteReady
              ? previewTerms(
                  quoted,
                  supply.sold[quoted.id] ?? 0,
                  quoteTarget.quantity,
                )
              : null,
          }
        : null,
    quoteFor,
    purchase,
    buy: (tileId, quantity, terms) => {
      const item = productOf(tileId);
      if (!item || terms.total === null) return;
      if (purchase && purchase.stage.kind !== "rejected") return;
      setPurchase({ tileId, quantity, terms, stage: { kind: "pending" } });
      later(COMMAND_MS, () => {
        const current = previewTerms(item, supply.sold[item.id] ?? 0, quantity);
        const coins = coinsOf(collection);
        if (
          current.total === null ||
          current.total !== terms.total ||
          coins < current.total
        ) {
          setPurchase({
            tileId,
            quantity,
            terms,
            stage: {
              kind: "rejected",
              message:
                current.total !== null && coins < current.total
                  ? "Not enough available coins."
                  : "Product no longer available.",
            },
          });
          return;
        }
        const total = current.total;
        updateCollection((existing) =>
          receive(existing, item, quantity, total),
        );
        setSupply((existing) => ({
          ...existing,
          sold: {
            ...existing.sold,
            [item.id]: (existing.sold[item.id] ?? 0) + quantity,
          },
        }));
        setPurchase(null);
        setCompleted({ kind: "bought", setName: item.setName, quantity });
      });
    },
    // The preview always answers, so nothing is left unconfirmed.
    checkPurchase: () => undefined,
    dismissPurchase: () =>
      setPurchase((current) =>
        current?.stage.kind === "rejected" ? null : current,
      ),
    ballot,
    saveBallot: (setIds) => {
      if (!isOpen || (save && save.kind !== "rejected")) return;
      setSave({ kind: "pending" });
      later(COMMAND_MS, () => {
        setSupply((existing) => ({ ...existing, vote: [...setIds] }));
        setSave(null);
        setCompleted({ kind: "voted" });
      });
    },
    checkBallot: () => undefined,
    dismissBallot: () =>
      setSave((current) => (current?.kind === "rejected" ? null : current)),
    completed,
    dismissCompleted: () => setCompleted(null),
  };
}
