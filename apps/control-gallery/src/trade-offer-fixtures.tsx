import type { OwnedPickerItem } from "@spellwake/ui";
import { ReferenceArtwork, type ArtworkCrop } from "./ReferenceArtwork";
import { tradePickerSamples, tradeCoinSample } from "./OwnedPickerSpecimen";
import type { View } from "./references";
export const lockedViews = [
  "TR04:locked-tray",
  "TR05:locked-tray",
  "TR06:locked-tray",
] as const;
export const editableViews = [
  "TR01:own-tray",
  "TR11:own-tray",
  "TR09:empty-tray",
  "TR09:availability",
  ...lockedViews,
] as const;
export type EditableView = (typeof editableViews)[number];
export function isEditableTrayView(view: unknown): view is EditableView {
  return editableViews.some((id) => id === view);
}
export type Holding = OwnedPickerItem & {
  maximum: number;
  quantity: number;
  draft: string;
};
export function exampleHoldings(view: EditableView): Holding[] {
  const dense = view === "TR11:own-tray";
  const art = (source: View, region: ArtworkCrop) => (
    <ReferenceArtwork source={source} region={region} />
  );
  const samples = [
    ...tradePickerSamples.cards,
    ...tradePickerSamples.packs,
  ].filter((row) => !dense || (row.id !== "opt" && row.id !== "island"));
  const locked = lockedViews.some((id) => id === view);
  const counts: Record<string, number> = {
    "TR01:own-tray": { bolt: 2, sol: 1 },
    "TR04:locked-tray": { bolt: 2, sol: 1 },
    "TR05:locked-tray": { bolt: 2, sol: 1 },
    "TR06:locked-tray": { bolt: 2, sol: 1 },
    "TR11:own-tray": { bolt: 4, sol: 1, counter: 2, fdn: 1 },
    "TR09:availability": { bolt: 2 },
    "TR09:empty-tray": {},
  }[view];
  const artSource = view === "TR09:empty-tray" ? "TR01:own-tray" : view;
  const overrides: Record<string, ArtworkCrop> = dense
    ? {
        bolt: [104, 327, 120, 65],
        sol: [104, 404, 120, 64],
        counter: [104, 554, 120, 63],
      }
    : locked
      ? { bolt: [101, 342, 155, 99], sol: [101, 467, 155, 100] }
      : view === "TR09:availability"
        ? { bolt: [814, 628, 161, 102] }
        : { bolt: [100, 337, 152, 97], sol: [100, 460, 152, 98] };
  const rows = samples.map((sample): Holding => {
    const pack = !("detail" in sample);
    const quantity = counts[sample.id] ?? 0;
    const crop = overrides[sample.id];
    const atLimit =
      sample.id === "bolt" && (view === "TR09:availability" || dense);
    const maximum = atLimit ? quantity : quantity + sample.available;
    return {
      id: sample.id,
      name: sample.name,
      ...("detail" in sample ? { detail: sample.detail } : {}),
      kind: pack ? "pack" : "card",
      artwork: crop
        ? art(artSource, crop)
        : art(pack ? "TR03:add-pack" : "TR02:add-card", sample.art),
      maximum,
      quantity,
      available: maximum - quantity,
      draft: String(quantity),
    };
  });
  if (dense) {
    const extra = (
      id: string,
      name: string,
      detail: string,
      quantity: number,
      maximum: number,
      rect: ArtworkCrop,
    ): Holding => ({
      id,
      name,
      detail,
      quantity,
      maximum,
      draft: String(quantity),
      kind: "card",
      available: maximum - quantity,
      artwork: art(view, rect),
    });
    rows.splice(
      2,
      0,
      extra("opt", "Opt", "M21  ·  Instant", 4, 8, [104, 480, 120, 63]),
    );
    rows.splice(
      4,
      0,
      extra(
        "island",
        "Island",
        "MH3  ·  Basic Land",
        20,
        30,
        [104, 627, 120, 63],
      ),
    );
  }
  const coins =
    view === "TR09:empty-tray"
      ? 0
      : view === "TR09:availability"
        ? tradeCoinSample.available
        : 250;
  rows.push({
    ...tradeCoinSample,
    maximum: tradeCoinSample.available,
    quantity: coins,
    draft: String(coins),
    available: tradeCoinSample.available - coins,
  });
  return rows;
}
