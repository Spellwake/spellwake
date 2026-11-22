import { useEffect, useState } from "react";
import {
  ManaCost,
  MatchActions,
  XValuePicker,
  type ManaSymbol,
} from "@spellwake/ui";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";
import paymentRed from "../../../.impeccable/review/open-play/approved-screens/implementation/shared-controls-batch-15/red-payment.png?inline";
import red from "../../../output/imagegen/guildhall-screen-set/components/assets/mana/R.svg?inline";
import one from "../../../output/imagegen/guildhall-screen-set/components/assets/mana/1.svg?inline";
import two from "../../../output/imagegen/guildhall-screen-set/components/assets/mana/2.svg?inline";
import three from "../../../output/imagegen/guildhall-screen-set/components/assets/mana/3.svg?inline";
import four from "../../../output/imagegen/guildhall-screen-set/components/assets/mana/4.svg?inline";

export const castingCostUses: ControlUse[] = [
  {
    id: "cast-payment",
    mapSection: "component",
    name: "Mana cost",
    description:
      "Lightning Bolt asks for one red mana. Blaze shows the total for the chosen X below the stepper.",
    screens: ["BC04:cost", "BC09:cost", "X:cost"],
  },
  {
    id: "cast-x",
    mapSection: "component",
    name: "X-value picker",
    description:
      "The approved Blaze concept combines the quantity control, Max and the mana-cost preview.",
    screens: ["X:cost"],
  },
  {
    id: "button",
    name: "Payment actions",
    description:
      "Cancel and Auto Pay use the shared match actions. Both are disabled while payment is submitting.",
    screens: ["BC04:payment", "BC05:payment", "BC09:payment"],
  },
];
const redCost = [{ name: "one red mana", src: paymentRed }];
const redPip = { name: "one red mana", src: red };
// Five Mountains in the approved example yield the following offered Blaze costs.
const blazeCosts: readonly (readonly ManaSymbol[])[] = [
  [],
  [one],
  [two],
  [three],
  [four],
].map((pips, x) => [
  ...pips.map((src) => ({ src, name: `${x} generic mana` })),
  redPip,
]);
const offered = blazeCosts.map((_, i) => i);
type PaymentState = "offered" | "submitting" | "accepted" | "cancelled";
export function CastingCostSpecimen({ view }: { view: View }) {
  const [x, setX] = useState(3);
  const fixedPending = view === "BC05:payment";
  const [state, setState] = useState<PaymentState>(
    fixedPending ? "submitting" : "offered",
  );
  const [, , width, height] = references[view].crop;
  useEffect(() => {
    if (state !== "submitting" || fixedPending) return;
    const timer = window.setTimeout(() => setState("accepted"), 900);
    return () => window.clearTimeout(timer);
  }, [state, fixedPending]);
  const closed = state === "accepted" || state === "cancelled";
  return (
    <>
      <div className="popup-stage">
        <div
          className="popup-scene casting-cost-scene"
          style={{ width, height }}
          data-specimen
        >
          {view === "X:cost" ? (
            <div className="blaze-picker">
              <XValuePicker
                value={x}
                offeredValues={offered}
                cost={blazeCosts[x]!}
                onChange={setX}
              />
            </div>
          ) : view.endsWith(":cost") ? (
            <div className="bolt-cost">
              <ManaCost label="Pay" size="payment" symbols={redCost} />
            </div>
          ) : (
            !closed && (
              <div className="bolt-payment">
                <MatchActions
                  layout="payment"
                  secondary={{
                    label: "Cancel",
                    onActivate: () => setState("cancelled"),
                  }}
                  primary={{
                    label: "Auto Pay",
                    onActivate: () => setState("submitting"),
                    pending: state === "submitting",
                  }}
                />
              </div>
            )
          )}
        </div>
      </div>
      {closed && (
        <div className="target-demo-actions">
          <span role="status">
            {state === "accepted" ? "Payment accepted" : "Casting cancelled"}
          </span>
          <button type="button" onClick={() => setState("offered")}>
            Reset example
          </button>
        </div>
      )}
    </>
  );
}
