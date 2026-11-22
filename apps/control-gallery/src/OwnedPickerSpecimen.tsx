import coin from "../../../assets/ui-study/coin.svg?inline";
import { useState } from "react";
import { OwnedItemPicker, type OwnedPickerItem } from "@spellwake/ui";
import { references, type View } from "./references";
import { ReferenceArtwork, type ArtworkCrop } from "./ReferenceArtwork";
import type { ControlUse } from "./UsageMap";

export const tradeCoinSample = {
  id: "coins",
  kind: "coin",
  name: "Coins",
  available: 1000,
  artwork: <img src={coin} alt="" width={66} />,
} satisfies OwnedPickerItem;
const fixtures = {
  "ML06:pick-printing": [
    {
      id: "m11",
      name: "Lightning Bolt",
      detail: "M11  ·  Nonfoil",
      available: 4,
      art: [477, 389, 80, 61],
    },
    {
      id: "2xm",
      name: "Lightning Bolt",
      detail: "2XM  ·  Nonfoil",
      available: 2,
      art: [477, 470, 80, 59],
    },
    {
      id: "2xm-foil",
      name: "Lightning Bolt",
      detail: "2XM  ·  Foil",
      available: 1,
      art: [477, 544, 80, 59],
    },
  ],
  "ML08:pick-pack": [
    {
      id: "fdn",
      name: "Foundations Play Booster",
      available: 4,
      art: [541, 245, 50, 62],
    },
    {
      id: "blb",
      name: "Bloomburrow Play Booster",
      available: 3,
      art: [541, 316, 50, 60],
    },
    {
      id: "dsk",
      name: "Duskmourn Play Booster",
      detail: "1 owned  ·  In a trade",
      available: 0,
      art: [541, 382, 50, 59],
    },
  ],
  "TR02:add-card": [
    {
      id: "bolt",
      name: "Lightning Bolt",
      detail: "M11  ·  Instant",
      available: 4,
      art: [496, 317, 103, 60],
    },
    {
      id: "sol",
      name: "Sol Ring",
      detail: "CMM  ·  Artifact",
      available: 3,
      art: [496, 389, 103, 60],
    },
    {
      id: "counter",
      name: "Counterspell",
      detail: "MH2  ·  Instant",
      available: 2,
      art: [496, 462, 103, 59],
    },
  ],
  "TR03:add-pack": [
    {
      id: "fdn",
      name: "Foundations Play Booster",
      available: 4,
      art: [509, 316, 69, 88],
    },
    {
      id: "mh3",
      name: "Modern Horizons 3 Play Booster",
      available: 2,
      art: [509, 412, 69, 88],
    },
  ],
} as const satisfies Record<
  string,
  readonly {
    id: string;
    name: string;
    detail?: string;
    available: number;
    art: ArtworkCrop;
  }[]
>;
export const tradePickerSamples = {
  cards: [
    ...fixtures["TR02:add-card"],
    {
      id: "opt",
      name: "Opt",
      detail: "M21  ·  Instant",
      available: 6,
      art: [496, 533, 103, 57],
    },
    {
      id: "island",
      name: "Island",
      detail: "Basic Land",
      available: 12,
      art: [496, 602, 103, 57],
    },
    {
      id: "mountain",
      name: "Mountain",
      detail: "Basic Land",
      available: 20,
      art: [496, 672, 103, 58],
    },
  ],
  packs: [
    ...fixtures["TR03:add-pack"],
    {
      id: "blb",
      name: "Bloomburrow Play Booster",
      available: 3,
      art: [509, 509, 69, 88],
    },
    {
      id: "dsk",
      name: "Duskmourn Play Booster",
      available: 1,
      art: [509, 605, 69, 88],
    },
  ],
} as const;
type PickerView = keyof typeof fixtures;
export function isOwnedPickerView(view: View): view is PickerView {
  return Object.hasOwn(fixtures, view);
}
const screens = Object.keys(fixtures) as PickerView[];
export const pickerUses: ControlUse[] = [
  {
    name: "Owned item picker",
    id: "item-picker",
    description:
      "Select one exact holding to list, or use the plus button to add one to your trade.",
    screens,
  },
  {
    name: "Item availability",
    id: "holding-picker",
    mapSection: "component",
    description:
      "Show how many are available, why a holding is unavailable, and how many have already been added to the trade.",
    screens,
  },
];
export function OwnedPickerSpecimen({ view }: { view: PickerView }) {
  const trade = view.startsWith("TR"),
    kind = view.endsWith("pack") ? "pack" : "card";
  const [selected, setSelected] = useState<string>(fixtures[view][0].id);
  const [holdings, setHoldings] = useState(() =>
    fixtures[view].map((row) => ({
      ...row,
      available: Number(row.available),
      added: view === "TR02:add-card" && row.id === "bolt" ? 2 : 0,
    })),
  );
  const [touched, setTouched] = useState(false);
  const items: OwnedPickerItem[] = holdings.map(({ art, ...data }) => ({
    ...data,
    kind,
    artwork: <ReferenceArtwork source={view} region={art} />,
  }));
  const totalAdded = holdings.reduce((sum, row) => sum + row.added, 0);
  const [, , width, height] = references[view].crop;
  function add(id: string) {
    setHoldings((current) =>
      current.map((row) =>
        row.id === id && row.available > 0
          ? { ...row, added: row.added + 1, available: row.available - 1 }
          : row,
      ),
    );
    setTouched(true);
  }
  const current = items.find((i) => i.id === selected);
  return (
    <>
      <div className="popup-stage">
        <div
          className="popup-scene inventory-stage"
          data-example={view}
          data-specimen
          style={{ width, height }}
        >
          {trade ? (
            <OwnedItemPicker
              label={`Add ${kind}s to trade`}
              mode="add"
              items={items}
              onAdd={add}
            />
          ) : (
            <OwnedItemPicker
              label={`Choose a ${kind === "card" ? "printing" : "pack"} to list`}
              mode="select"
              items={items}
              selectedId={selected}
              onSelect={(id) => {
                setSelected(id);
                setTouched(true);
              }}
            />
          )}
        </div>
      </div>
      <div className="inventory-feedback" role="status">
        {touched &&
          (trade
            ? `${totalAdded} ${kind}${totalAdded === 1 ? "" : "s"} added`
            : `Selected ${current?.name}${current?.detail ? " · " + current.detail : ""}`)}
      </div>
    </>
  );
}
