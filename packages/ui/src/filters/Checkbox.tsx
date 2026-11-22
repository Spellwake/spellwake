import type { InputHTMLAttributes, ReactNode } from "react";
import * as s from "./filters.css";
export interface CheckboxProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "type" | "className" | "checked" | "defaultChecked" | "onChange" | "children"
> {
  checked: boolean;
  label: ReactNode;
  onCheckedChange: (value: boolean) => void;
  treatment?: "filled" | "outline";
  className?: string;
}
/** A labeled native checkbox. A caller can supply a full tile as its label content. */
export function Checkbox({
  checked,
  label,
  onCheckedChange,
  treatment = "filled",
  className,
  ...input
}: CheckboxProps) {
  return (
    <label className={[s.boxLabel, className].filter(Boolean).join(" ")}>
      <input
        {...input}
        type="checkbox"
        checked={checked}
        className={[s.box, treatment === "outline" && s.outlined]
          .filter(Boolean)
          .join(" ")}
        onChange={(event) => onCheckedChange(event.currentTarget.checked)}
      />
      {label}
    </label>
  );
}
