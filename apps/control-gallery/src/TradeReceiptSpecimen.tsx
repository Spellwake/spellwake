import { TradeReceipt } from "@spellwake/ui";
import { traySample } from "./TradeTraySpecimen";
import {
  exampleHoldings,
  isEditableTrayView,
  type EditableView,
} from "./trade-offer-fixtures";
import { references } from "./references";
import type { ControlUse } from "./UsageMap";

export type ReceiptExample = {
  source: EditableView;
  quantities: Record<string, number>;
};

/** History stores the fixture identifiers so refresh and Back can reconstruct a settled example. */
export function receiptFromHistory(value: unknown): ReceiptExample | undefined {
  if (
    !value ||
    typeof value !== "object" ||
    !("source" in value) ||
    !isEditableTrayView(value.source) ||
    !("quantities" in value) ||
    !value.quantities ||
    typeof value.quantities !== "object"
  )
    return;
  const quantities: Record<string, number> = {};
  for (const [id, count] of Object.entries(value.quantities)) {
    if (typeof count !== "number" || !Number.isSafeInteger(count) || count < 0)
      return;
    quantities[id] = count;
  }
  return { source: value.source, quantities };
}
export const receiptUses: ControlUse[] = [
  {
    name: "Trade receipt",
    id: "trade-trays",
    mapSection: "component",
    description:
      "The completed trade reuses both read-only trays, their item rows and the room’s primary button.",
    screens: [
      "TR08:receipt",
      "TR08:gave-tray",
      "TR08:received-tray",
      "TR08:receipt-coins",
      "TR06:actions",
    ],
  },
];
export function receiptContributions(example: ReceiptExample | undefined) {
  const gave = traySample("TR08:gave-tray");
  const received = traySample("TR08:received-tray");
  if (example) {
    gave.items = exampleHoldings(example.source).flatMap((row) => {
      const count = example.quantities[row.id] ?? 0;
      return count > 0 ? [{ ...row, quantity: count }] : [];
    });
    if (example.source === "TR09:empty-tray") received.items = [];
  }
  return { gave, received };
}
export function TradeReceiptSpecimen({
  example,
  onDone,
}: {
  example: ReceiptExample | undefined;
  onDone: () => void;
}) {
  const { gave, received } = receiptContributions(example);
  const [, , width, height] = references["TR08:receipt"].crop;
  return (
    <div className="popup-stage">
      <div
        data-specimen
        className="popup-scene receipt-example"
        style={{ width, minHeight: height }}
      >
        <TradeReceipt
          gave={gave}
          received={received}
          onDone={onDone}
          headingId="receipt-heading"
        />
      </div>
    </div>
  );
}
