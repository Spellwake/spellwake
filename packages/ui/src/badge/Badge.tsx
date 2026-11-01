import { type ReactNode, type ReactElement } from "react";
import * as s from "../controls.css";

export function Badge({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: "neutral" | "danger";
}): ReactElement {
  return (
    <span className={`${s.badge} ${tone === "danger" ? s.badgeDanger : ""}`}>
      {children}
    </span>
  );
}
