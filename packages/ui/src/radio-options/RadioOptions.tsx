import { useId, type ReactElement } from "react";
import { RadioGroup } from "radix-ui";
import * as s from "../controls.css";

export function RadioOptions({
  label,
  value,
  onValueChange,
  options,
}: {
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  options: { value: string; label: string }[];
}): ReactElement {
  const id = useId();
  return (
    <RadioGroup.Root
      className={s.radioGroup}
      aria-label={label}
      value={value}
      onValueChange={onValueChange}
    >
      {options.map((option) => (
        <label
          key={option.value}
          htmlFor={`${id}-${option.value}`}
          className={s.choiceLabel}
        >
          <RadioGroup.Item
            id={`${id}-${option.value}`}
            value={option.value}
            className={s.radio}
          >
            <RadioGroup.Indicator className={s.radioDot} />
          </RadioGroup.Item>
          {option.label}
        </label>
      ))}
    </RadioGroup.Root>
  );
}
