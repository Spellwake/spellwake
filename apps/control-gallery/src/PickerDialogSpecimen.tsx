import { useRef, useState } from "react";
import {
  ActionButton,
  TradeItemPicker,
  type OwnedPickerItem,
} from "@spellwake/ui";
import { ReferenceArtwork } from "./ReferenceArtwork";
import { tradePickerSamples, tradeCoinSample } from "./OwnedPickerSpecimen";
import { references } from "./references";
import type { ControlUse } from "./UsageMap";
const pickerViews = [
  "TR02:picker-dialog",
  "TR03:picker-dialog",
  "TR09:no-matches",
  "TR09:empty-category",
] as const;
type PickerView = (typeof pickerViews)[number];
export function isPickerDialogView(view: string | null): view is PickerView {
  return pickerViews.some((id) => id === view);
}
export const pickerDialogUses: ControlUse[] = [
  {
    id: "holding-picker",
    mapSection: "component",
    name: "Add items dialog",
    description:
      "Choose a card, sealed pack or coins to add directly to the offer.",
    screens: [...pickerViews, "TR01:own-tray", "TR11:own-tray"],
  },
];
export function PickerDialogSpecimen({ view }: { view: PickerView }) {
  const [, , width, height] = references[view].crop;
  return (
    <div className="popup-stage">
      <iframe
        className="confirm-preview"
        key={view}
        data-specimen
        title={references[view].state}
        src={`?picker=${encodeURIComponent(view)}`}
        style={{ width, height }}
      />
    </div>
  );
}
export function PickerDialogExample({ view }: { view: PickerView }) {
  const button = useRef<HTMLButtonElement>(null),
    [open, setOpen] = useState(true);
  const [initialSearch, setInitialSearch] = useState(
    view === "TR09:no-matches" ? "zzzz" : "",
  );
  const cards = view === "TR09:empty-category" ? [] : tradePickerSamples.cards;
  const [items, setItems] = useState<OwnedPickerItem[]>(() => [
    ...[...cards, ...tradePickerSamples.packs].map((row): OwnedPickerItem => {
      const card = "detail" in row;
      return {
        ...row,
        kind: card ? "card" : "pack",
        added: row.id === "bolt" && !view.startsWith("TR09") ? 2 : 0,
        artwork: (
          <ReferenceArtwork
            source={card ? "TR02:add-card" : "TR03:add-pack"}
            region={row.art}
          />
        ),
      };
    }),
    tradeCoinSample,
  ]);
  return (
    <div className="confirmation-example">
      <ActionButton ref={button} onClick={() => setOpen(true)}>
        Add items
      </ActionButton>
      {open && (
        <TradeItemPicker
          items={items}
          initialTab={view === "TR03:picker-dialog" ? "pack" : "card"}
          initialSearch={initialSearch}
          returnFocusRef={button}
          onClose={() => {
            setOpen(false);
            setInitialSearch("");
          }}
          onAdd={(id) =>
            setItems((prev) =>
              prev.map((row) =>
                row.id === id && row.available > 0
                  ? {
                      ...row,
                      available: row.available - 1,
                      added: (row.added ?? 0) + 1,
                    }
                  : row,
              ),
            )
          }
        />
      )}
    </div>
  );
}
