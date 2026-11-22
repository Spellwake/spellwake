import { useState } from "react";
import {
  ListingRow,
  ListingItemSummary,
  type ListingItem,
} from "@spellwake/ui";
import coin from "../../../assets/ui-study/coin.svg?inline";
import { ReferenceArtwork, type ArtworkCrop } from "./ReferenceArtwork";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

const listingViews = [
  "ML01:cards",
  "ML01:packs",
  "ML02:own",
  "ML04:buy-item",
  "ML09:cancel-item",
] as const satisfies readonly View[];
type ListingView = (typeof listingViews)[number];
export function isListingView(view: View): view is ListingView {
  return references[view].surface === "Listing items";
}
export const listingUses: ControlUse[] = [
  {
    id: "listing-board",
    mapSection: "component",
    name: "Listing row",
    description:
      "Select an offer in public card and pack listings or your own active listings.",
    screens: listingViews.slice(0, 3),
  },
  {
    id: "item-row",
    name: "Listing item summary",
    description:
      "Printing, quantity and seller are shared by the list rows and purchase summary. Cancellation shows the offered lot and its total.",
    screens: listingViews,
  },
];
type Example = { id: string; item: ListingItem; seller: string; total: number };
function examples(view: ListingView): Example[] {
  const art = (region: ArtworkCrop) => (
    <ReferenceArtwork source={view} region={region} />
  );
  const bolt: Example = {
    id: "bolt",
    seller: "Mira",
    total: 500,
    item: {
      kind: "card",
      name: "Lightning Bolt",
      setCode: "M11",
      finish: "Nonfoil",
      quantity: 4,
      artwork: art(
        view === "ML04:buy-item"
          ? [470, 342, 200, 280]
          : view === "ML09:cancel-item"
            ? [440, 466, 92, 94]
            : [266, 244, 92, 97],
      ),
    },
  };
  if (view === "ML01:packs")
    return [
      {
        id: "foundations",
        seller: "Finch",
        total: 6000,
        item: {
          kind: "pack",
          name: "Foundations Play Booster",
          quantity: 2,
          artwork: art([273, 707, 84, 113]),
        },
      },
      {
        id: "bloomburrow",
        seller: "Mira",
        total: 3600,
        item: {
          kind: "pack",
          name: "Bloomburrow Play Booster",
          quantity: 1,
          artwork: art([272, 832, 88, 121]),
        },
      },
    ];
  if (view === "ML02:own")
    return [
      { ...bolt, seller: "You" },
      {
        id: "sol",
        seller: "You",
        total: 700,
        item: {
          kind: "card",
          name: "Sol Ring",
          setCode: "CMM",
          finish: "Nonfoil",
          quantity: 1,
          artwork: art([266, 364, 94, 95]),
        },
      },
    ];
  if (view === "ML01:cards")
    return [
      bolt,
      {
        id: "counterspell",
        seller: "Ash",
        total: 350,
        item: {
          kind: "card",
          name: "Counterspell",
          setCode: "MH2",
          finish: "Nonfoil",
          quantity: 2,
          artwork: art([266, 361, 92, 96]),
        },
      },
    ];
  return [bolt];
}
export function ListingSpecimen({ view }: { view: ListingView }) {
  const offers = examples(view),
    first = offers[0]!,
    buy = view === "ML04:buy-item",
    cancel = view === "ML09:cancel-item";
  const [selected, setSelected] = useState<string | null>(
    view === "ML01:packs" ? null : first.id,
  );
  const [inspected, inspect] = useState(false);
  const active = offers.find((o) => o.id === selected);
  const [, , width, height] = references[view].crop;
  return (
    <>
      <div className="popup-stage">
        <div
          data-specimen
          className="popup-scene market-stage"
          data-example={view}
          style={{ width, height }}
        >
          {buy ? (
            <ListingItemSummary
              item={first.item}
              seller={first.seller}
              layout="detail"
            />
          ) : cancel ? (
            <ListingItemSummary
              item={first.item}
              price={{ amount: first.total, iconSrc: coin }}
            />
          ) : (
            offers.map((o) => (
              <ListingRow
                key={o.id}
                item={o.item}
                seller={o.seller}
                price={{ amount: o.total, iconSrc: coin }}
                selected={selected === o.id}
                onSelect={() => {
                  setSelected(o.id);
                  inspect(true);
                }}
              />
            ))
          )}
        </div>
      </div>
      {!buy && !cancel && (
        <div role="status" className="market-inspected">
          {inspected && active && (
            <>
              <span className="market-inspected-label">Selected listing</span>
              <ListingItemSummary
                item={active.item}
                seller={active.seller}
                price={{ amount: active.total, iconSrc: coin }}
              />
            </>
          )}
        </div>
      )}
    </>
  );
}
