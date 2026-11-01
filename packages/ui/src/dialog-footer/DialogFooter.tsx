import { type ReactNode, type ReactElement } from "react";
import * as s from "../controls.css";

export function DialogFooter({
  children,
}: {
  children: ReactNode;
}): ReactElement {
  return <div className={s.dialogFooter}>{children}</div>;
}
