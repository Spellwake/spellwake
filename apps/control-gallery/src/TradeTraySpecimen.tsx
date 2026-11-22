import {
  TradeTray,
  type TradeTrayItem,
  type TradeTrayProps,
} from "@spellwake/ui";
import coin from "../../../assets/ui-study/coin.svg?inline";
import { ReferenceArtwork, type ArtworkCrop } from "./ReferenceArtwork";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

const crops = {
  "TR01:partner-tray": {
    portrait: [822, 234, 70, 69],
    card: [823, 337, 157, 98],
    pack: [847, 463, 119, 175],
  },
  "TR04:partner-tray": {
    portrait: [835, 238, 71, 69],
    card: [836, 342, 159, 100],
    pack: [861, 470, 120, 179],
  },
  "TR08:received-tray": {
    portrait: [835, 238, 71, 69],
    card: [836, 342, 159, 100],
    pack: [861, 470, 120, 179],
  },
  "TR08:gave-tray": {
    portrait: [107, 238, 70, 69],
    card: [101, 342, 155, 99],
    secondCard: [101, 467, 155, 100],
  },
} as const satisfies Record<string, Record<string, ArtworkCrop>>;
type TrayView = keyof typeof crops;
export function isTradeTrayView(view: View): view is TrayView {
  return Object.hasOwn(crops, view);
}
export const tradeTrayUses: ControlUse[] = [
  {
    id: "trade-trays",
    name: "Trade tray",
    mapSection: "component",
    description:
      "The partner’s offer and both sides of the receipt share a portrait header and a bounded item list.",
    screens: [
      "TR01:partner-tray",
      "TR04:partner-tray",
      "TR08:gave-tray",
      "TR08:received-tray",
    ],
  },
];
export function traySample(view: TrayView) {
  const sample = crops[view];
  const art = (region: ArtworkCrop) => (
    <ReferenceArtwork source={view} region={region} />
  );
  const items: TradeTrayItem[] =
    "secondCard" in sample
      ? [
          {
            id: "m11-bolt",
            kind: "card",
            name: "Lightning Bolt",
            detail: "M11  ·  Instant",
            quantity: 2,
            artwork: art(sample.card),
          },
          {
            id: "cmm-ring",
            kind: "card",
            name: "Sol Ring",
            detail: "CMM  ·  Artifact",
            quantity: 1,
            artwork: art(sample.secondCard),
          },
          {
            id: "coins",
            kind: "coin",
            name: "Coins",
            quantity: 250,
            artwork: <img src={coin} width="72" alt="" />,
          },
        ]
      : [
          {
            id: "mh2-counterspell",
            kind: "card",
            name: "Counterspell",
            detail: "MH2  ·  Instant",
            quantity: 1,
            artwork: art(sample.card),
          },
          {
            id: "fdn-pack",
            kind: "pack",
            name: "Foundations Play Booster",
            quantity: 2,
            artwork: art(sample.pack),
          },
        ];
  return { items, portrait: art(sample.portrait) };
}
export function TradeTraySpecimen({ view }: { view: TrayView }) {
  const [, , width, height] = references[view].crop;
  const state =
    view === "TR08:gave-tray" || view === "TR08:received-tray"
      ? ({
          kind: "receipt",
          direction: view === "TR08:gave-tray" ? "gave" : "received",
        } as const)
      : ({
          kind: "offer",
          participant: "Mira",
          status: view === "TR01:partner-tray" ? "editing" : "ready",
        } as const);
  const props: TradeTrayProps = {
    ...state,
    ...traySample(view),
  };
  return (
    <div className="popup-stage">
      <div
        data-specimen
        className="popup-scene tray-example"
        style={{ width, height }}
      >
        <TradeTray {...props} />
      </div>
    </div>
  );
}
