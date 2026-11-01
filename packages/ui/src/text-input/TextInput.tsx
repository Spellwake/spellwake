import {
  useId,
  type ComponentProps,
  type ReactNode,
  type ReactElement,
} from "react";
import * as s from "../controls.css";

type FieldProps = Omit<ComponentProps<"input">, "size"> & {
  label: string;
  help?: string;
  error?: string;
  leading?: ReactNode;
};

/** Labeled field with stable helper/error associations and quiet focus styling. */
export function TextInput({
  label,
  help,
  error,
  leading,
  id: suppliedId,
  className = "",
  disabled = false,
  ...props
}: FieldProps): ReactElement {
  const generatedId = useId();
  const id = suppliedId ?? generatedId;
  const describedBy = error || help ? `${id}-help` : undefined;
  return (
    <div className={`${s.fieldWrap} ${className}`}>
      <label className={s.label} htmlFor={id}>
        {label}
      </label>
      <div
        className={s.inputShell}
        data-error={!!error}
        data-disabled={disabled}
      >
        {leading}
        <input
          {...props}
          id={id}
          disabled={disabled}
          className={s.input}
          aria-invalid={!!error}
          aria-describedby={describedBy}
        />
      </div>
      {describedBy && (
        <div
          id={describedBy}
          className={`${s.helper} ${error ? s.errorText : ""}`}
        >
          {error ?? help}
        </div>
      )}
    </div>
  );
}
