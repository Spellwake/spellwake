import { useState } from "react";
import { TargetConnection, TargetOption, TargetPrompt } from "@spellwake/ui";
import { references, type View } from "./references";
import type { ControlUse } from "./UsageMap";
import elves from "../../../assets/ui-review/open-play/approved-screens/implementation/shared-controls-batch-14/elves.webp?inline";
import bears from "../../../assets/ui-review/open-play/approved-screens/implementation/shared-controls-batch-14/bears.webp?inline";
import angel from "../../../assets/ui-review/open-play/approved-screens/implementation/shared-controls-batch-14/angel.webp?inline";
import felt from "../../../assets/ui-review/open-play/approved-screens/implementation/shared-controls-batch-14/felt.webp?inline";

const screens = ["BC02:targeting", "MT01:targeting", "MT04:targeting"] as const;
export const targetingUses: ControlUse[] = [
  {
    id: "cast-target",
    mapSection: "component",
    name: "Target prompt",
    description:
      "Lightning Bolt asks for one target. Common Bond numbers its two creature choices.",
    screens,
  },
  {
    id: "selection-offer",
    name: "Legal target highlight",
    description:
      "The cyan rim follows the selectable object throughout the offer. A creature chosen for Common Bond’s first instruction can be chosen again.",
    screens,
  },
];
const cards = [
  { name: "Llanowar Elves", src: elves, x: 30, skew: -1.8 },
  { name: "Grizzly Bears", src: bears, x: 172, skew: 0.7 },
  { name: "Serra Angel", src: angel, x: 311, skew: 2.2 },
];
export function TargetingSpecimen({ view }: { view: View }) {
  const bond = view.startsWith("MT");
  const startsSecond = view.startsWith("MT04");
  const [step, setStep] = useState(startsSecond ? 2 : 1);
  // The approved second-step camera shows the connection to a Serra Angel outside this crop.
  const [first, setFirst] = useState<{
    name: string;
    x: number;
    y: number;
  } | null>(startsSecond ? { name: "Serra Angel", x: -218, y: 480 } : null);
  const [result, setResult] = useState<string[] | null>(null);
  const [, , width, height] = references[view].crop;
  function choose(card: (typeof cards)[number]) {
    if (bond && step === 1) {
      setFirst({ name: card.name, x: card.x + 59, y: 278 });
      setStep(2);
    } else setResult(first ? [first.name, card.name] : [card.name]);
  }
  return (
    <>
      <div className="popup-stage">
        <div
          className="popup-scene targeting-scene"
          data-specimen
          style={{ width, height, backgroundImage: "url(" + felt + ")" }}
        >
          {!result && (
            <div className="target-instruction">
              <TargetPrompt
                instruction={bond ? "Choose a creature" : "Choose a target"}
                {...(bond ? { step: { current: step, total: 2 } } : {})}
              />
            </div>
          )}
          {cards.map((card) => (
            <div
              key={card.name}
              className="target-card"
              style={{
                left: card.x,
                transform: `perspective(650px) rotateX(13deg) skewX(${card.skew}deg)`,
              }}
            >
              <TargetOption
                label={"Choose " + card.name}
                eligible={!result}
                onChoose={() => choose(card)}
              >
                <img src={card.src} alt="" draggable={false} />
              </TargetOption>
            </div>
          ))}
          {first && !result && (
            <TargetConnection
              from={first.x < 0 ? { x: 483, y: 239 } : { x: 742, y: 277 }}
              to={first.x < 0 ? { x: -25, y: 296 } : first}
              bend={first.x < 0 ? 53 : 75}
            />
          )}
        </div>
      </div>
      <div className="target-demo-actions">
        {step === 2 && !result && (
          <button type="button" onClick={() => setStep(1)}>
            Back
          </button>
        )}
        {result && (
          <>
            <span role="status">{result.join(" → ")}</span>
            <button
              type="button"
              onClick={() => {
                setResult(null);
                setFirst(null);
                setStep(1);
              }}
            >
              Reset example
            </button>
          </>
        )}
      </div>
    </>
  );
}
