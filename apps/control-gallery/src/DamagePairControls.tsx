import { QuantityControl, adjustDamageAllocation } from "@spellwake/ui";
import { Fragment } from "react";

export type DamagePair = [number, number];
interface PairProps {
  values: DamagePair;
  total: number;
  minimum: number;
  names: readonly [string, string];
  onChange: (next: DamagePair) => void;
  showNames?: boolean;
}

/** Two-recipient fixture shared by the quantity and allocation previews. */
export function DamagePairControls({
  values,
  total,
  minimum,
  names,
  onChange,
  showNames = false,
}: PairProps) {
  return (
    <>
      {([0, 1] as const).map((i) => {
        const donor = i === 0 ? 1 : 0;
        const full = values[0] + values[1] === total;
        function adjust(delta: 1 | -1) {
          onChange(adjustDamageAllocation(values, i, delta, total, minimum));
        }
        const control = (
          <QuantityControl
            label={`damage to ${names[i]}`}
            value={values[i]}
            treatment="pill"
            canDecrease={values[i] > minimum}
            canIncrease={!full || values[donor] > minimum}
            onDecrease={() => adjust(-1)}
            onIncrease={() => adjust(1)}
          />
        );
        return (
          <Fragment key={i}>
            {showNames ? (
              <div>
                <span>{names[i]}</span>
                {control}
              </div>
            ) : (
              control
            )}
          </Fragment>
        );
      })}
    </>
  );
}
