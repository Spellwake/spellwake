import { type ReactNode, type ReactElement } from "react";
import * as s from "../controls.css";

export function Alert({
  children,
  role = "status",
}: {
  children: ReactNode;
  role?: "status" | "alert";
}): ReactElement {
  return (
    <div className={s.alert} role={role}>
      {children}
    </div>
  );
}
