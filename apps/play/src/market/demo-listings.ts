import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAtom } from "jotai";
import {
  useBuilderCatalogue,
  type BuilderCard,
} from "../data/builder-catalogue";
import {
  coinsOf,
  useDemoCollection,
  type DemoCollection,
} from "../collection/demo-collection";
import { costColors, identityOf } from "../collection/demo-cards";
import {
  marketAtom,
  useDemoListingHolds,
  cardAssetId,
  type MyListing,
} from "./demo-market-store";
import type {
  Completed,
  ListableAsset,
  Listing,
  ListingCard,
  ListingPack,
  ListingsSource,
  Operation,
} from "./listing-sources";

/**
 * The local preview of Market Listings. Other people's listings are examples
 * drawn from the preview catalogue; buying one moves its cards or packs into
 * the preview collection and its total out of the preview wallet. Your own
 * listings hold their cards or packs until you cancel them; nobody in the
 * preview buys them. Every command answers after a moment and is accepted
 * unless the preview can tell it would fail. Nothing reaches an account.
 */

const COMMAND_MS = 700;
const NO_LONGER = "Listing no longer available.";

/** The example market, newest first. Cards name the preview catalogue. */
const SELLERS = {
  mira: "Mira",
  ash: "Ash",
  rowan: "Rowan",
  juniper: "Juniper",
  finch: "Finch",
} as const;
type Seed = {
  id: string;
  seller: keyof typeof SELLERS;
  quantity: number;
  total: number;
} & ({ card: string; foil?: boolean } | { pack: string; code: string });
const SEED: readonly Seed[] = [
  {
    id: "bolt",
    card: "Lightning Bolt",
    seller: "mira",
    quantity: 4,
    total: 500,
  },
  {
    id: "guide",
    card: "Goblin Guide",
    foil: true,
    seller: "ash",
    quantity: 2,
    total: 350,
  },
  { id: "shock", card: "Shock", seller: "rowan", quantity: 4, total: 220 },
  {
    id: "krenko",
    card: "Krenko, Mob Boss",
    seller: "juniper",
    quantity: 1,
    total: 700,
  },
  {
    id: "fdn",
    pack: "Foundations",
    code: "FDN",
    seller: "finch",
    quantity: 2,
    total: 6000,
  },
  {
    id: "blb",
    pack: "Bloomburrow",
    code: "BLB",
    seller: "mira",
    quantity: 1,
    total: 3600,
  },
  {
    id: "chieftain",
    card: "Goblin Chieftain",
    seller: "ash",
    quantity: 2,
    total: 260,
  },
  {
    id: "mountain",
    card: "Mountain",
    seller: "rowan",
    quantity: 20,
    total: 100,
  },
  {
    id: "mh3",
    pack: "Modern Horizons 3",
    code: "MH3",
    seller: "juniper",
    quantity: 1,
    total: 4800,
  },
  { id: "vial", card: "Aether Vial", seller: "finch", quantity: 1, total: 900 },
];

export const cardItem = (card: BuilderCard, foil: boolean): ListingCard => ({
  kind: "card",
  cardId: card.name,
  name: card.name,
  setCode: card.printing?.set ?? null,
  setName: card.printing?.set ?? "Unknown set",
  number: card.printing?.number ?? null,
  finish: foil ? "Foil" : "Nonfoil",
  rarity: null,
  faceUrl: card.faceUrl,
  artUrl: card.artUrl,
  type: card.type,
  rules: card.rules,
  manaValue: card.manaValue,
  colors: costColors(card),
  identity: identityOf(card),
  legality: card.legality,
});
const packItem = (setName: string, code: string): ListingPack => ({
  kind: "pack",
  product: `${setName} Play Booster`,
  setName,
  setCode: code,
});

/**
 * What you own that a listing could offer, less what trades and your listings
 * hold. As everywhere, only what can be used now is counted, with no word
 * about what is held.
 */
function assetsOf(
  collection: DemoCollection,
  find: (name: string) => BuilderCard | undefined,
  holds: Readonly<Record<string, number>>,
): ListableAsset[] {
  const cards = Object.entries(collection.cards).flatMap(([name, held]) => {
    const card = find(name);
    if (!card) return [];
    return [false, true].flatMap((foil): ListableAsset[] => {
      const owned = foil ? held.foil : held.nonfoil;
      if (owned < 1) return [];
      const id = cardAssetId(name, foil);
      const listed = holds[id] ?? 0;
      return [
        {
          id,
          item: cardItem(card, foil),
          available: Math.max(0, owned - listed),
        },
      ];
    });
  });
  const packs = collection.packs.map((pack): ListableAsset => {
    const listed = holds[pack.id] ?? 0;
    return {
      id: pack.id,
      item: packItem(pack.setName, pack.setCode),
      available: Math.max(0, pack.owned - pack.held - listed),
    };
  });
  return [
    ...cards.sort(
      (a, b) =>
        (a.item.kind === "card" ? a.item.name : "").localeCompare(
          b.item.kind === "card" ? b.item.name : "",
        ) || a.id.localeCompare(b.id),
    ),
    ...packs,
  ];
}

/** Moves a bought lot into the preview collection and its total out of the wallet. */
function receive(collection: DemoCollection, listing: Listing): DemoCollection {
  const { item } = listing;
  const coins = coinsOf(collection) - (listing.total ?? 0);
  if (item.kind === "card") {
    const held = collection.cards[item.name] ?? { nonfoil: 0, foil: 0 };
    const foil = item.finish === "Foil";
    return {
      ...collection,
      coins,
      cards: {
        ...collection.cards,
        [item.name]: foil
          ? { ...held, foil: held.foil + listing.quantity }
          : { ...held, nonfoil: held.nonfoil + listing.quantity },
      },
    };
  }
  const owned = collection.packs.some((pack) => pack.setName === item.setName);
  return {
    ...collection,
    coins,
    packs: owned
      ? collection.packs.map((pack) =>
          pack.setName === item.setName
            ? { ...pack, owned: pack.owned + listing.quantity }
            : pack,
        )
      : [
          ...collection.packs,
          {
            id: `preview-pack-${(item.setCode ?? item.setName).toLowerCase()}`,
            product: item.product,
            setName: item.setName,
            setCode: item.setCode ?? item.setName.slice(0, 3).toUpperCase(),
            owned: listing.quantity,
            held: 0,
          },
        ],
  };
}

/** Preview Listings. */
export function useDemoListings(): ListingsSource {
  const { catalogue, hasFailed } = useBuilderCatalogue();
  const [collection, updateCollection] = useDemoCollection();
  const [market, setMarket] = useAtom(marketAtom);
  const holds = useDemoListingHolds();
  const [operation, setOperation] = useState<Operation | null>(null);
  const [completed, setCompleted] = useState<Completed | null>(null);
  const [failureDismissed, setFailureDismissed] = useState(false);
  const timers = useRef<number[]>([]);
  useEffect(
    () => () => {
      for (const timer of timers.current) window.clearTimeout(timer);
    },
    [],
  );
  const later = useCallback((run: () => void) => {
    timers.current.push(window.setTimeout(run, COMMAND_MS));
  }, []);
  const find = useCallback(
    (name: string) => catalogue?.find(name),
    [catalogue],
  );

  const assets = useMemo(
    () => (catalogue ? assetsOf(collection, find, holds) : null),
    [catalogue, collection, find, holds],
  );
  const listings = useMemo((): Listing[] => {
    if (!catalogue) return [];
    const others = SEED.flatMap((seed, index): Listing[] => {
      if (market.sold.includes(seed.id)) return [];
      const item =
        "card" in seed
          ? (() => {
              const card = find(seed.card);
              return card ? cardItem(card, seed.foil ?? false) : null;
            })()
          : packItem(seed.pack, seed.code);
      if (!item) return [];
      return [
        {
          id: `preview-listing-${seed.id}`,
          revision: "1",
          item,
          quantity: seed.quantity,
          total: seed.total,
          totalText: String(seed.total),
          seller: { id: seed.seller, name: SELLERS[seed.seller] },
          isMine: false,
          listedAt: SEED.length - index,
        },
      ];
    });
    const own = market.mine.flatMap((mine: MyListing): Listing[] => {
      const asset = assetsOf(collection, find, {}).find(
        (candidate) => candidate.id === mine.assetId,
      );
      if (!asset) return [];
      return [
        {
          id: mine.id,
          revision: "1",
          item: asset.item,
          quantity: mine.quantity,
          total: mine.total,
          totalText: String(mine.total),
          seller: { id: "you", name: "You" },
          isMine: true,
          listedAt: mine.listedAt,
        },
      ];
    });
    return [...own, ...others];
  }, [catalogue, find, market, collection]);

  const isBusy = operation !== null && operation.stage.kind !== "rejected";
  if (!catalogue && !hasFailed) return { status: "loading" };
  return {
    status: "ready",
    listings,
    facets: {
      formats: [
        "Standard",
        "Pioneer",
        "Modern",
        "Legacy",
        "Vintage",
        "Commander",
        "Pauper",
      ],
      color: true,
      type: true,
      // The preview catalogue records no rarity.
      rarity: false,
      manaValue: true,
      sorts: ["newest", "price-asc", "price-desc", "name-asc"],
    },
    isAppending: false,
    isUnread: hasFailed,
    failure: hasFailed && !failureDismissed ? "Couldn’t load listings." : null,
    dismissFailure: () => setFailureDismissed(true),
    balance: coinsOf(collection),
    assets,
    canTrade: true,
    operation,
    completed,
    dismissCompleted: () => setCompleted(null),
    buy: (listingId) => {
      const listing = listings.find((entry) => entry.id === listingId);
      if (!listing || listing.isMine || isBusy) return;
      setOperation({ kind: "buy", listing, stage: { kind: "pending" } });
      later(() => {
        const seed = SEED.find((s) => `preview-listing-${s.id}` === listingId);
        if (!seed || (listing.total ?? 0) > coinsOf(collection)) {
          setOperation({
            kind: "buy",
            listing,
            stage: { kind: "rejected", message: NO_LONGER },
          });
          return;
        }
        updateCollection((current) => receive(current, listing));
        setMarket((current) => ({
          ...current,
          sold: [...current.sold, seed.id],
        }));
        setOperation(null);
        setCompleted({ kind: "bought", listing });
      });
    },
    create: (assetId, quantity, total) => {
      const asset = assets?.find((entry) => entry.id === assetId);
      if (!asset || isBusy) return;
      setOperation({
        kind: "create",
        asset,
        quantity,
        total,
        stage: { kind: "pending" },
      });
      later(() => {
        if (quantity > asset.available) {
          setOperation({
            kind: "create",
            asset,
            quantity,
            total,
            stage: {
              kind: "rejected",
              message: `Only ${asset.available} available.`,
            },
          });
          return;
        }
        const id = `preview-mine-${crypto.randomUUID()}`;
        setMarket((current) => ({
          ...current,
          mine: [
            { id, assetId, quantity, total, listedAt: Date.now() },
            ...current.mine,
          ],
        }));
        setOperation(null);
        setCompleted({ kind: "created", listingId: id });
      });
    },
    cancel: (listingId) => {
      const listing = listings.find((entry) => entry.id === listingId);
      if (!listing?.isMine || isBusy) return;
      setOperation({ kind: "cancel", listing, stage: { kind: "pending" } });
      later(() => {
        setMarket((current) => ({
          ...current,
          mine: current.mine.filter((mine) => mine.id !== listingId),
        }));
        setOperation(null);
        setCompleted({ kind: "cancelled", listingId });
      });
    },
    // The preview always answers, so nothing is left unconfirmed.
    checkStatus: () => undefined,
    dismissOperation: () =>
      setOperation((current) =>
        current?.stage.kind === "rejected" ? null : current,
      ),
  };
}
