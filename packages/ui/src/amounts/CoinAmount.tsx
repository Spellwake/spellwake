import { AmountDigits } from "./AmountDigits";
import * as s from "./amounts.css";

export interface CoinAmountProps {
  amount: number | null;
  iconSrc: string;
  unavailableLabel?: string;
}
/** Render the supplied whole-coin amount immediately, or an accessible unavailable state. */
export function CoinAmount({
  amount,
  iconSrc,
  unavailableLabel = "Coin amount unavailable",
}: CoinAmountProps) {
  const known = amount !== null && Number.isSafeInteger(amount) && amount >= 0;
  return (
    <span className={s.coin}>
      {known && <img src={iconSrc} alt="" className={s.icon} />}
      <span aria-hidden="true">
        <AmountDigits value={known ? amount : "–"} />
      </span>
      <span className={s.srOnly}>
        {known
          ? `${amount} Spellwake ${amount === 1 ? "coin" : "coins"}`
          : unavailableLabel}
      </span>
    </span>
  );
}
