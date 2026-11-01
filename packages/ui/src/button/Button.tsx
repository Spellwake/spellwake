import { type ComponentProps, type ReactElement } from "react";
import { LoaderCircle } from "lucide-react";
import * as s from "../controls.css";

export type ButtonVariant = keyof typeof s.variant;

export type ButtonSize = keyof typeof s.size;

export type ButtonProps = ComponentProps<"button"> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  selected?: boolean;
  pill?: boolean;
  previewState?: "hover" | "pressed" | "focus";
};

/** Shared action anatomy. Selection is opt-in and independent of hover. */
export function Button({
  variant = "secondary",
  size = "md",
  loading = false,
  selected,
  pill = false,
  previewState,
  className = "",
  disabled,
  children,
  type = "button",
  ...props
}: ButtonProps): ReactElement {
  return (
    <button
      {...props}
      type={type}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      data-selected={selected}
      data-preview-state={previewState}
      className={`${s.control} ${s.size[size]} ${s.variant[variant]} ${pill ? s.pill : ""} ${className}`}
    >
      {loading && <LoaderCircle aria-hidden className={s.spinner} />}
      <span>{children}</span>
    </button>
  );
}
