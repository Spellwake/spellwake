import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import { LoaderCircle } from "lucide-react";
import * as s from "../controls.css";

export interface ActionButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  ref?: Ref<HTMLButtonElement>;
  icon?: ReactNode;
  emphasis?: "neutral" | "primary";
  treatment?: "toolbar" | "header" | "form" | "compact-form";
  pending?: boolean;
  pendingIndicator?: "ring" | "dots";
}

export function ActionButton({
  children,
  icon,
  emphasis = "neutral",
  treatment = "toolbar",
  pending = false,
  pendingIndicator = "ring",
  disabled,
  type = "button",
  className,
  ...props
}: ActionButtonProps) {
  const classes = [
    s.button,
    emphasis === "primary" && s.emphasis,
    treatment === "header" && s.headerButton,
    (treatment === "form" || treatment === "compact-form") &&
      (emphasis === "neutral" ? s.formButton : s.formPrimary),
    treatment === "compact-form" && s.compactFormButton,
    className,
  ];
  return (
    <button
      {...props}
      type={type}
      disabled={disabled || pending}
      aria-busy={pending || props["aria-busy"]}
      className={classes.filter(Boolean).join(" ")}
    >
      {(pending || icon) && (
        <span aria-hidden="true" className={s.icon}>
          {pending ? (
            pendingIndicator === "dots" ? (
              <svg viewBox="0 0 24 24" className={s.spinner} focusable="false">
                <circle
                  cx="12"
                  cy="12"
                  r="9"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="3"
                  strokeLinecap="round"
                  strokeDasharray=".7 4.44"
                />
              </svg>
            ) : (
              <LoaderCircle className={s.spinner} />
            )
          ) : (
            icon
          )}
        </span>
      )}
      {children}
    </button>
  );
}
