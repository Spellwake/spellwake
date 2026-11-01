import { useId, type ComponentProps, type ReactElement } from "react";
import * as s from "../controls.css";

export function TextArea({
  label,
  help,
  error,
  ...props
}: ComponentProps<"textarea"> & {
  label: string;
  help?: string;
  error?: string;
}): ReactElement {
  const id = useId();
  return (
    <div className={s.fieldWrap}>
      <label className={s.label} htmlFor={id}>
        {label}
      </label>
      <div className={s.inputShell} data-error={!!error}>
        <textarea
          {...props}
          id={id}
          className={s.textarea}
          aria-invalid={!!error}
          aria-describedby={error || help ? `${id}-help` : undefined}
        />
      </div>
      {(error || help) && (
        <div
          className={`${s.helper} ${error ? s.errorText : ""}`}
          id={`${id}-help`}
        >
          {error ?? help}
        </div>
      )}
    </div>
  );
}
