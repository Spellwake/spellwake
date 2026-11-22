import { useId, useRef, useState } from "react";
import { DamagePairControls, type DamagePair } from "./DamagePairControls";
import { ArrowRight } from "lucide-react";
import { QuantityControl, CoinAmount } from "@spellwake/ui";
import coin from "../../../assets/ui-study/coin.svg?inline";
import type { View } from "./references";
import type { ControlUse } from "./UsageMap";

export const amountUses: ControlUse[] = [
  {
    id: "quantity",
    name: "Quantity control",
    approvedCount: 32,
    description:
      "Deck builder, Trade, Listings, Supply, divided spell damage and the X-value concept.",
    screens: ["SP-03", "SP-10", "DB-04", "DC03"],
  },
  {
    id: "coin-amount",
    name: "Spellwake coin amount",
    approvedCount: 22,
    description:
      "Trade offers, Listings prices, Supply quotes, Predictions bets and returns, and League rewards.",
    screens: ["SP-03", "RS-01"],
  },
];
// Exact approved quotes for this example with 420 packs remaining.
const packQuotes = new Map([
  [1, 1000],
  [2, 2003],
]);

function PackQuantity({ invalid }: { invalid: boolean }) {
  const [draft, setDraft] = useState(invalid ? "1.5" : "1");
  const id = useId();
  const n = Number(draft);
  const valid = /^\d+$/.test(draft) && n >= 1 && n <= 1000;
  const total = valid ? (packQuotes.get(n) ?? null) : null;
  const error = valid ? "" : "Enter a whole number from 1 to 1000.";
  return (
    <>
      <div className="preference-stage purchase-stage">
        <div
          data-specimen
          className={"purchase-rows" + (invalid ? " invalid-rows" : "")}
        >
          <div className="amount-line">
            <span>Quantity</span>
            <QuantityControl
              label="pack quantity"
              value={n}
              edit={{
                draft,
                onChange: setDraft,
                error,
                ...(error ? { descriptionId: id } : {}),
              }}
              canDecrease={Number.isFinite(n) && Math.ceil(n) > 1}
              canIncrease={Number.isFinite(n) && Math.floor(n) < 420}
              onDecrease={() => setDraft(String(Math.ceil(n) - 1))}
              onIncrease={() =>
                setDraft(String(Math.max(1, Math.floor(n) + 1)))
              }
            />
          </div>
          {error && (
            <p id={id} className="amount-error">
              {error}
            </p>
          )}
          <div className="amount-line">
            <span>Total</span>
            <CoinAmount amount={total} iconSrc={coin} />
          </div>
        </div>
      </div>
      <div className="readout" role="status">
        {!valid
          ? "Quantity needs a whole number."
          : n > 420
            ? "420 packs available."
            : total === null
              ? "No approved quote for this quantity in the example."
              : `${n} ${n === 1 ? "pack" : "packs"} · ${total} coins`}
      </div>
    </>
  );
}
function DeckCopies() {
  const [quantity, setQuantity] = useState(1),
    [draft, setDraft] = useState("1");
  const errorId = useId();
  const removedRef = useRef<HTMLButtonElement>(null);
  const valid = /^\d+$/.test(draft) && Number(draft) <= 999;
  function update(next: number) {
    setQuantity(next);
    setDraft(String(next));
    if (next === 0) requestAnimationFrame(() => removedRef.current?.focus());
  }
  const error = valid ? "" : "Enter a whole number from 0 to 999.";
  return (
    <>
      <div className="preference-stage deck-amount-stage">
        <div data-specimen>
          {quantity > 0 ? (
            <>
              <QuantityControl
                label="deck copies"
                value={quantity}
                treatment="joined"
                edit={{
                  draft,
                  onChange: setDraft,
                  onCancel: () => setDraft(String(quantity)),
                  onCommit: () => {
                    if (valid && Number(draft) !== quantity)
                      update(Number(draft));
                  },
                  error,
                  ...(error ? { descriptionId: errorId } : {}),
                }}
                removeOnDecrease={quantity === 1}
                canDecrease
                canIncrease={quantity < 999}
                onDecrease={() => update(quantity - 1)}
                onIncrease={() => update(quantity + 1)}
              />
              {error && (
                <p id={errorId} className="amount-error">
                  {error}
                </p>
              )}
            </>
          ) : (
            <span>Card removed</span>
          )}
        </div>
      </div>
      <div className="readout">
        <span role="status">
          {quantity} committed {quantity === 1 ? "copy" : "copies"}
        </span>
        <button type="button" ref={removedRef} onClick={() => update(1)}>
          Reset example
        </button>
      </div>
    </>
  );
}
function SpellDamageSplit() {
  const total = 3,
    min = 1;
  const [values, setValues] = useState<DamagePair>([2, 1]);
  const remaining = total - values[0] - values[1];
  const names: [string, string] = ["Elf", "Grizzly Bears"];
  return (
    <>
      <div className={"preference-stage allocation-stage " + "spell-stage"}>
        <div data-specimen className="allocation-pair">
          <DamagePairControls
            values={values}
            total={total}
            minimum={min}
            names={names}
            onChange={setValues}
          />
        </div>
      </div>
      <div className="readout" role="status">
        {total - remaining} / {total} assigned
      </div>
    </>
  );
}
function ResultAmounts() {
  return (
    <div className="preference-stage result-stage">
      <div className="result-amounts" data-specimen>
        <div>
          Your bet
          <CoinAmount amount={200} iconSrc={coin} />
        </div>
        <ArrowRight aria-hidden />
        <div>
          Returned
          <CoinAmount amount={500} iconSrc={coin} />
        </div>
      </div>
    </div>
  );
}
export function AmountSpecimen({ view }: { view: View }) {
  if (view === "RS-01") return <ResultAmounts />;
  if (view === "DC03") return <SpellDamageSplit />;
  if (view === "DB-04") return <DeckCopies />;
  return <PackQuantity invalid={view === "SP-10"} />;
}
