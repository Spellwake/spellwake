import type { ReactElement } from "react";
import { useSearchParams } from "react-router-dom";
import { useBuilderCatalogue } from "../../data/builder-catalogue";
import { cardItem } from "../../market/demo-listings";
import type {
  ListableAsset,
  Listing,
  ListingsSource,
  Operation,
  ReadyListings,
} from "../../market/listing-sources";
import { ListingsPage } from "../market/ListingsPage";

/**
 * Development only: Listings in states the preview cannot reach on its own,
 * for the stress checks and review captures. `?state=` names one.
 */
const noop = (): void => undefined;

export function MarketProbe(): ReactElement {
  const [params] = useSearchParams();
  const state = params.get("state") ?? "buy-unconfirmed";
  const { catalogue } = useBuilderCatalogue();
  const bolt = catalogue?.find("Lightning Bolt");
  const guide = catalogue?.find("Goblin Guide");
  if (!bolt || !guide) return <ListingsPage source={{ status: "loading" }} />;
  const listing = (
    id: string,
    item: Listing["item"],
    quantity: number,
    total: number,
    seller: string,
    isMine = false,
  ): Listing => ({
    id,
    revision: "1",
    item,
    quantity,
    total,
    totalText: String(total),
    seller: { id: seller, name: isMine ? "You" : seller },
    isMine,
    listedAt: total,
  });
  const theirs = listing("bolt", cardItem(bolt, false), 4, 500, "Mira");
  const mine = listing("mine", cardItem(bolt, false), 4, 500, "you", true);
  const pack = listing(
    "fdn",
    {
      kind: "pack",
      product: "Foundations Play Booster",
      setName: "Foundations",
      setCode: "FDN",
    },
    2,
    6000,
    "Finch",
  );
  const asset: ListableAsset = {
    id: "card:Lightning Bolt:nonfoil",
    item: cardItem(bolt, false),
    available: 4,
  };
  const ready = (overrides: Partial<ReadyListings>): ListingsSource => ({
    status: "ready",
    listings: [
      theirs,
      listing("guide", cardItem(guide, true), 2, 350, "Ash"),
      pack,
    ],
    facets: {
      formats: ["Standard", "Modern", "Legacy", "Commander"],
      color: true,
      type: true,
      rarity: false,
      manaValue: true,
      sorts: ["newest", "price-asc", "price-desc", "name-asc"],
    },
    isAppending: false,
    isUnread: false,
    failure: null,
    dismissFailure: noop,
    balance: 8000,
    assets: [asset],
    canTrade: true,
    operation: null,
    completed: null,
    dismissCompleted: noop,
    buy: noop,
    create: noop,
    cancel: noop,
    checkStatus: noop,
    dismissOperation: noop,
    ...overrides,
  });
  const create = (stage: Operation["stage"]): Operation => ({
    kind: "create",
    asset,
    quantity: 4,
    total: 500,
    stage,
  });
  const sources: Record<string, ListingsSource> = {
    loading: { status: "loading" },
    "buy-pending": ready({
      operation: { kind: "buy", listing: theirs, stage: { kind: "pending" } },
    }),
    "buy-unconfirmed": ready({
      operation: {
        kind: "buy",
        listing: theirs,
        stage: { kind: "unconfirmed", isChecking: false },
      },
    }),
    "buy-checking": ready({
      operation: {
        kind: "buy",
        listing: theirs,
        stage: { kind: "unconfirmed", isChecking: true },
      },
    }),
    "buy-rejected": ready({
      operation: {
        kind: "buy",
        listing: theirs,
        stage: { kind: "rejected", message: "Listing no longer available." },
      },
    }),
    "create-pending": ready({ operation: create({ kind: "pending" }) }),
    "create-unconfirmed": ready({
      operation: create({ kind: "unconfirmed", isChecking: false }),
    }),
    "cancel-unconfirmed": ready({
      listings: [mine],
      operation: {
        kind: "cancel",
        listing: mine,
        stage: { kind: "unconfirmed", isChecking: false },
      },
    }),
    mine: ready({ listings: [mine, theirs] }),
    poor: ready({ balance: 100 }),
    "read-failed": ready({
      listings: [],
      isUnread: true,
      failure: "Couldn’t load listings.",
    }),
    empty: ready({ listings: [] }),
    "no-assets": ready({ assets: [] }),
    "pack-selected": ready({ listings: [pack] }),
    "not-allowed": ready({ canTrade: false }),
  };
  return <ListingsPage source={sources[state] ?? ready({})} />;
}
