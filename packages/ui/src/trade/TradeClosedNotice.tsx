import { ChevronLeft } from "lucide-react";
import { motion } from "motion/react";
import { useId } from "react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./closed-notice.css";

export type TradeClosure = "canceled" | "unavailable";
export const tradeClosureTitle = {
  canceled: "Trade canceled.",
  unavailable: "Trade unavailable.",
} as const satisfies Record<TradeClosure, string>;

export type TradeClosedNoticeProps = {
  reason: TradeClosure;
  onBack: () => void;
  headingId?: string;
};

/** A closed room has one destination, supplied by the surface that opened it. */
export function TradeClosedNotice({
  reason,
  onBack,
  headingId,
}: TradeClosedNoticeProps) {
  const id = useId();
  const titleId = headingId ?? id;
  const animate = useAllowsControlMotion();
  return (
    <section className={s.panel} aria-labelledby={titleId} data-reason={reason}>
      <motion.div
        className={s.copy}
        initial={animate ? { opacity: 0.6, y: 2 } : false}
        animate={{ opacity: 1, y: 0 }}
        transition={{
          duration: animate ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        }}
      >
        <h2 className={s.title} id={titleId} tabIndex={-1}>
          {tradeClosureTitle[reason]}
        </h2>
        {reason === "canceled" && (
          <p className={s.detail}>Items are available in your collections.</p>
        )}
      </motion.div>
      <footer className={s.footer}>
        <ActionButton
          className={s.back}
          icon={<ChevronLeft aria-hidden="true" />}
          onClick={onBack}
        >
          Back
        </ActionButton>
      </footer>
    </section>
  );
}
