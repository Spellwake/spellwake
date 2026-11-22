import { useId, type ReactNode } from "react";
import { hidden } from "../controls.css";
import * as s from "./preferences.css";

export interface RadioOption {
  value: string;
  label: string;
  decoration?: ReactNode;
  /** Optional rich face for a chip. The label still names the native input. */
  chipContent?: ReactNode;
  disabled?: boolean;
}
export interface RadioGroupProps {
  label: string;
  value: string;
  options: readonly RadioOption[];
  onValueChange: (value: string) => void;
  name?: string;
  treatment?: "rows" | "chips";
  disabled?: boolean;
}
/** One choice among the supplied options. The caller owns eligibility and saving. */
export function RadioGroup({
  label,
  value,
  options,
  onValueChange,
  name,
  treatment = "rows",
  disabled,
}: RadioGroupProps) {
  const uniqueName = useId();
  const chips = treatment === "chips";
  return (
    <fieldset className={chips ? s.chips : s.options} disabled={disabled}>
      <legend className={hidden}>{label}</legend>
      {options.map((option) => (
        <label
          className={chips ? s.chipLabel : s.optionLabel}
          key={option.value}
        >
          <input
            type="radio"
            aria-label={chips ? option.label : undefined}
            className={chips ? hidden : s.radio}
            name={name ?? uniqueName}
            value={option.value}
            checked={value === option.value}
            disabled={option.disabled}
            onChange={(event) => onValueChange(event.currentTarget.value)}
          />
          {chips ? (
            <span className={s.chip} aria-hidden>
              {option.chipContent ?? option.label}
            </span>
          ) : (
            <span>
              {option.label}
              {option.decoration && (
                <span aria-hidden="true"> {option.decoration}</span>
              )}
            </span>
          )}
        </label>
      ))}
    </fieldset>
  );
}
