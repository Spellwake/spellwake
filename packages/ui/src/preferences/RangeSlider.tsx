import { useRef, type CSSProperties } from "react";
import { Minus, Plus } from "lucide-react";
import * as s from "./preferences.css";

export interface RangeSliderProps {
  label: string;
  id?: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onValueChange: (value: number) => void;
  valueText?: string;
  showStepButtons?: boolean;
  disabled?: boolean;
  autoFocus?: boolean;
}

/** A controlled numeric range. Optional end buttons use the input's native step grid. */
export function RangeSlider({
  label,
  id,
  value,
  min,
  max,
  step,
  onValueChange,
  valueText,
  showStepButtons = false,
  disabled,
  autoFocus,
}: RangeSliderProps) {
  const ref = useRef<HTMLInputElement>(null);
  const fraction =
    max > min ? Math.max(0, Math.min(1, (value - min) / (max - min))) : 0;
  function adjust(direction: "decrease" | "increase") {
    const input = ref.current;
    if (!input) return;
    if (direction === "decrease") input.stepDown();
    else input.stepUp();
    if (input.valueAsNumber !== value) onValueChange(input.valueAsNumber);
  }
  const noHigherStep = value + step > max + Math.abs(step) * 1e-9;
  return (
    <div
      className={[s.rangeWrap, showStepButtons && s.adjusted]
        .filter(Boolean)
        .join(" ")}
    >
      {showStepButtons && (
        <button
          type="button"
          aria-label={`Decrease ${label.toLowerCase()}`}
          className={s.stepButton}
          disabled={disabled || value <= min}
          onClick={() => adjust("decrease")}
        >
          <Minus aria-hidden="true" />
        </button>
      )}
      <input
        id={id}
        ref={ref}
        aria-label={label}
        aria-valuetext={valueText}
        type="range"
        value={value}
        min={min}
        max={max}
        step={step}
        className={s.range}
        style={{ "--sw-range-progress": `${fraction * 100}%` } as CSSProperties}
        autoFocus={autoFocus}
        disabled={disabled}
        onChange={(event) => onValueChange(event.currentTarget.valueAsNumber)}
      />
      {showStepButtons && (
        <button
          type="button"
          aria-label={`Increase ${label.toLowerCase()}`}
          className={s.stepButton}
          disabled={disabled || noHigherStep}
          onClick={() => adjust("increase")}
        >
          <Plus aria-hidden="true" />
        </button>
      )}
    </div>
  );
}
