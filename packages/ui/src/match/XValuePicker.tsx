import { ActionButton } from "../button/ActionButton";
import { QuantityControl } from "../amounts/QuantityControl";
import { ManaCost, type ManaSymbol } from "./ManaCost";
import * as s from "./castingCost.css";

export interface XValuePickerProps {
  value: number;
  /** Offered values in increasing order, including the current value. */
  offeredValues: readonly number[];
  cost: readonly ManaSymbol[];
  disabled?: boolean;
  onChange: (value: number) => void;
}

/** Changes a draft X only. The caller supplies the legal values and repriced total. */
export function XValuePicker({
  value,
  offeredValues,
  cost,
  disabled = false,
  onChange,
}: XValuePickerProps) {
  const index = offeredValues.indexOf(value);
  const previous = offeredValues[index - 1];
  const next = index < 0 ? undefined : offeredValues[index + 1];
  const maximum = offeredValues.at(-1);
  return (
    <div className={s.picker}>
      <div className={s.row}>
        <span aria-hidden="true" className={s.x}>
          X
        </span>
        <QuantityControl
          label="X"
          value={value}
          treatment="ivory-pill"
          disabled={disabled}
          canDecrease={previous !== undefined}
          canIncrease={next !== undefined}
          onDecrease={() => previous !== undefined && onChange(previous)}
          onIncrease={() => next !== undefined && onChange(next)}
        />
        <ActionButton
          className={s.max}
          disabled={disabled || maximum === undefined || value === maximum}
          aria-label="Set X to maximum"
          onClick={() => maximum !== undefined && onChange(maximum)}
        >
          Max
        </ActionButton>
      </div>
      <ManaCost label="Cost" symbols={cost} />
    </div>
  );
}
