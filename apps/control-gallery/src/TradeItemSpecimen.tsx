import { useState, useRef } from "react";
import { ActionButton, TradeItemRow, TradeQuantityEditor } from "@spellwake/ui";
import { ReferenceArtwork, type ArtworkCrop } from "./ReferenceArtwork";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";
import coin from "../../../assets/ui-study/coin.svg?inline";

type Item = {
  kind: "card" | "coin" | "pack";
  name: string;
  detail?: string;
  quantity: number;
  art?: ArtworkCrop;
};
const bolt = {
  kind: "card",
  name: "Lightning Bolt",
  detail: "M11  ·  Instant",
  quantity: 2,
} as const;
const fixtures = {
  "TR01:edit-item": { ...bolt, art: [100, 337, 152, 97] },
  "TR01:partner-item": {
    kind: "pack",
    name: "Foundations Play Booster",
    quantity: 2,
    art: [847, 463, 119, 175],
  },
  "TR04:locked-item": { ...bolt, art: [101, 342, 155, 99] },
  "TR08:receipt-item": { ...bolt, art: [101, 342, 155, 99] },
  "TR08:receipt-coins": { kind: "coin", name: "Coins", quantity: 250 },
} as const satisfies Partial<Record<View, Item>>;
type ItemView = keyof typeof fixtures;
export function isTradeItemView(view: View): view is ItemView {
  return Object.hasOwn(fixtures, view);
}
export const tradeItemUses: ControlUse[] = [
  {
    id: "trade-trays",
    name: "Trade item row",
    mapSection: "component",
    description:
      "Card, pack and coin entries in both offers and the completed receipt.",
    screens: [
      "TR01:edit-item",
      "TR01:partner-item",
      "TR04:locked-item",
      "TR08:receipt-item",
      "TR08:receipt-coins",
    ],
  },
  {
    id: "quantity",
    name: "Trade quantity editor",
    description:
      "Edit a copy count up to the amount you own, or remove the item from your offer.",
    screens: ["TR01:edit-item", "TR09:availability", "TR11:own-tray"],
  },
];
export function TradeItemSpecimen({ view }: { view: ItemView }) {
  const item: Item = fixtures[view],
    editable = view === "TR01:edit-item";
  const [quantity, setQuantity] = useState<number>(item.quantity),
    [draft, setDraft] = useState(String(item.quantity));
  const resetButton = useRef<HTMLButtonElement>(null);
  const [, , width, height] = references[view].crop;
  function commit(n: number) {
    setQuantity(n);
    setDraft(String(n));
    if (n === 0) requestAnimationFrame(() => resetButton.current?.focus());
  }
  return (
    <>
      <div className="popup-stage">
        <div
          data-specimen
          className="popup-scene trade-item-stage"
          data-kind={item.kind}
          style={{ width, minHeight: height }}
        >
          {quantity === 0 ? (
            <span role="status">No items added.</span>
          ) : (
            <TradeItemRow
              {...item}
              quantity={quantity}
              artwork={
                item.art ? (
                  <ReferenceArtwork source={view} region={item.art} />
                ) : (
                  <img src={coin} width="72" alt="" />
                )
              }
              editor={
                editable ? (
                  <TradeQuantityEditor
                    label="Lightning Bolt M11 quantity"
                    quantity={quantity}
                    draft={draft}
                    available={4}
                    onDraftChange={setDraft}
                    onCommit={commit}
                    clearLabel="Remove Lightning Bolt M11"
                  />
                ) : undefined
              }
            />
          )}
        </div>
      </div>
      {editable && (
        <div className="readout">
          <ActionButton ref={resetButton} onClick={() => commit(item.quantity)}>
            Reset example
          </ActionButton>
        </div>
      )}
    </>
  );
}
