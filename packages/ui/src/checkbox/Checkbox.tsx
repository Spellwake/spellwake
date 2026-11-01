import { useId, type ReactElement } from "react";
import { Checkbox as RCheckbox } from "radix-ui";
import { Check } from "lucide-react";
import * as s from "../controls.css";

export function Checkbox({
  label,
  checked,
  onCheckedChange,
  disabled = false,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}): ReactElement {
  const id = useId();
  return (
    <label htmlFor={id} className={s.choiceLabel}>
      <RCheckbox.Root
        id={id}
        className={s.checkbox}
        checked={checked}
        onCheckedChange={(value) => onCheckedChange(value === true)}
        disabled={disabled}
      >
        <RCheckbox.Indicator>
          <Check size={15} strokeWidth={2} />
        </RCheckbox.Indicator>
      </RCheckbox.Root>
      {label}
    </label>
  );
}
