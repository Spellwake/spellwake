import { type ReactElement } from "react";
import { LoaderCircle } from "lucide-react";
import * as s from "../controls.css";

export function Spinner(): ReactElement {
  return <LoaderCircle className={s.spinner} aria-label="Loading" size={20} />;
}
