import { useId, useRef, type InputHTMLAttributes } from "react";
import { Search, X } from "lucide-react";
import * as s from "../controls.css";
import { designPixels } from "../design-pixels";

export interface TextFieldProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "value" | "onChange" | "defaultValue" | "type" | "size" | "children"
> {
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  type?: "text" | "search" | "url" | "email" | "password";
  hideLabel?: boolean;
  treatment?: "default" | "header" | "compact-form";
  hint?: string;
  error?: string;
}

export function TextField({
  id,
  label,
  value,
  onValueChange,
  type = "text",
  hideLabel = false,
  treatment = "default",
  hint,
  error,
  readOnly,
  disabled,
  className,
  "aria-describedby": describedBy,
  ...props
}: TextFieldProps) {
  const generated = useId();
  const inputId = id ?? generated;
  const description = error || hint;
  const descriptions = [describedBy, description && inputId + "-description"]
    .filter(Boolean)
    .join(" ");
  const input = useRef<HTMLInputElement>(null);
  const search = type === "search";
  return (
    <div className={[s.field, className].filter(Boolean).join(" ")}>
      <label className={hideLabel ? s.hidden : s.label} htmlFor={inputId}>
        {label}
      </label>
      <div
        className={[
          s.frame,
          search && s.searchFrame,
          treatment === "header" && s.headerFrame,
          treatment === "compact-form" && s.compactField,
        ]
          .filter(Boolean)
          .join(" ")}
        data-disabled={disabled}
      >
        {search && (
          <span className={s.icon} aria-hidden="true">
            <Search />
          </span>
        )}
        <input
          {...props}
          id={inputId}
          type={type}
          ref={input}
          className={s.input}
          value={value}
          disabled={disabled}
          readOnly={readOnly}
          aria-invalid={error ? true : props["aria-invalid"]}
          aria-describedby={descriptions || undefined}
          onChange={(event) => onValueChange(event.currentTarget.value)}
        />
        {search && value && !readOnly && !disabled && (
          <button
            type="button"
            className={s.clear}
            aria-label={`Clear ${label.toLowerCase()}`}
            onClick={() => {
              onValueChange("");
              input.current?.focus();
            }}
          >
            <X size={designPixels(25)} aria-hidden="true" />
          </button>
        )}
      </div>
      {description && (
        <span
          id={inputId + "-description"}
          className={[s.hint, error && s.errorText].filter(Boolean).join(" ")}
        >
          {description}
        </span>
      )}
    </div>
  );
}
