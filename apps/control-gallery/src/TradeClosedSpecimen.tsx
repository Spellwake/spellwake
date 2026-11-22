import { TradeClosedNotice, type TradeClosure } from "@spellwake/ui";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";

export const closedScenes = {
  "TR10:canceled": "canceled",
  "TR10:unavailable": "unavailable",
} as const satisfies Partial<Record<View, TradeClosure>>;
export function isClosedView(view: View): view is keyof typeof closedScenes {
  return Object.hasOwn(closedScenes, view);
}
export const tradeClosedUses: ControlUse[] = [
  {
    name: "Closed trade notice",
    id: "trade-consent",
    mapSection: "component",
    description:
      "A closed room shows its outcome and a single Back action. Confirming cancellation opens the canceled state.",
    screens: [
      "TR10:canceled",
      "TR10:unavailable",
      "TR07:confirm",
      "TR04:actions",
    ],
  },
];

export function TradeClosedSpecimen({
  view,
  onBack,
}: {
  view: keyof typeof closedScenes;
  onBack: () => void;
}) {
  const width = references[view].crop[2];
  return (
    <div className="popup-stage">
      <div
        data-specimen
        className="popup-scene closed-trade-example"
        style={{ width }}
      >
        <TradeClosedNotice
          reason={closedScenes[view]}
          onBack={onBack}
          headingId="closed-trade-heading"
        />
      </div>
    </div>
  );
}
