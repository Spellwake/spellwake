import { type ReactNode, type ReactElement } from "react";
import * as s from "../controls.css";

export function FieldStack({
  children,
}: {
  children: ReactNode;
}): ReactElement {
  return <div className={s.stack}>{children}</div>;
}
