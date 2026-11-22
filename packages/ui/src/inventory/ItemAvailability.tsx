import { AmountDigits } from "../amounts/AmountDigits";
import * as s from "./picker.css";

export interface ItemAvailabilityProps {
  id?: string;
  available: number;
  added?: number;
}

/** Available is the remaining quantity. Added is the quantity already in the trade. */
export function ItemAvailability({
  id,
  available,
  added = 0,
}: ItemAvailabilityProps) {
  return (
    <span id={id} className={s.availability}>
      {added > 0 && (
        <span className={s.added}>
          Added <AmountDigits value={added} />
        </span>
      )}
      <span className={s.stock}>
        <AmountDigits value={available} /> available
      </span>
    </span>
  );
}
