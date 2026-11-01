import { type ReactNode, type ReactElement } from "react";
import * as s from "../controls.css";
import { Button } from "../button/Button";
import type { ButtonProps } from "../button/Button";

export type IconButtonProps = Omit<ButtonProps, "children"> & {
  label: string;
  children: ReactNode;
};

/** Every icon action supplies an accessible name and a stable hit area. */
export function IconButton({
  label,
  children,
  className = "",
  variant = "ghost",
  ...props
}: IconButtonProps): ReactElement {
  return (
    <Button
      {...props}
      aria-label={label}
      variant={variant}
      className={`${s.iconOnly} ${className}`}
    >
      {children}
    </Button>
  );
}
