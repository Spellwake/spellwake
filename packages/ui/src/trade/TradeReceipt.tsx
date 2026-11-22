import { Check } from "lucide-react";
import { motion } from "motion/react";
import type { ReactNode } from "react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { TradeTray, type TradeTrayItem } from "./TradeTray";
import { primary } from "./actions.css";
import * as s from "./receipt.css";
import { designPixels } from "../design-pixels";

type Contribution = { portrait: ReactNode; items: readonly TradeTrayItem[] };
export type TradeReceiptProps = {
  gave: Contribution;
  received: Contribution;
  onDone: () => void;
  headingId?: string;
};

/** Renders settled quantities. Done returns control to the calling surface. */
export function TradeReceipt({
  gave,
  received,
  onDone,
  headingId,
}: TradeReceiptProps) {
  const animate = useAllowsControlMotion();
  return (
    <section className={s.receipt} aria-label="Trade receipt">
      <h2 className={s.heading} tabIndex={-1} id={headingId}>
        <motion.span
          className={s.mark}
          aria-hidden="true"
          initial={animate ? { opacity: 0, scale: 0.9 } : false}
          animate={{ opacity: 1, scale: 1 }}
          transition={{
            duration: animate ? controlMotion.settle : 0,
            ease: controlMotion.ease,
          }}
        >
          <Check size={designPixels(42)} strokeWidth={2.2} />
        </motion.span>
        Trade complete.
      </h2>
      <div className={s.trays}>
        <TradeTray kind="receipt" direction="gave" {...gave} />
        <TradeTray kind="receipt" direction="received" {...received} />
      </div>
      <ActionButton className={primary + " " + s.done} onClick={onDone}>
        Done
      </ActionButton>
    </section>
  );
}
