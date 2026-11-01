import { useId, type ReactElement } from "react";
import { Switch as RSwitch } from "radix-ui";
import * as s from "../controls.css";

export function Switch({
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
    <label className={s.choiceLabel} htmlFor={id}>
      <RSwitch.Root
        id={id}
        className={s.switchRoot}
        checked={checked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
      >
        <RSwitch.Thumb className={s.switchThumb} />
      </RSwitch.Root>
      {label}
    </label>
  );
}
