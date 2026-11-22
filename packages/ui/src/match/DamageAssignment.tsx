import { AmountDigits } from "../amounts/AmountDigits";
import * as s from "./damage.css";
export interface DamageAssignmentReadoutProps {
  total: number;
  assigned: number;
}
/** Divided spell damage uses the caster’s total across all selected recipients. */
export function DamageAssignmentReadout({
  total,
  assigned,
}: DamageAssignmentReadoutProps) {
  return (
    <div role="status" aria-atomic="true" className={s.prompt}>
      Divide {total} damage <span aria-hidden>·</span>
      <AmountDigits value={`${assigned}/${total}`} />
    </div>
  );
}
