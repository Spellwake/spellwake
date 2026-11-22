import { AnimatePresence, motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";

/** Animate the newly displayed value. No intermediate count or outgoing duplicate. */
export function AmountDigits({ value }: { value: number | string }) {
  const animate = useAllowsControlMotion();
  return (
    <AnimatePresence initial={false}>
      <motion.span
        key={value}
        style={{ display: "inline-block" }}
        initial={animate ? { y: 2, opacity: 0.74 } : false}
        animate={{ y: 0, opacity: 1 }}
        transition={{
          duration: animate ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        }}
      >
        {value}
      </motion.span>
    </AnimatePresence>
  );
}
