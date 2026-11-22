import { motion } from "motion/react";
import { type ReactNode } from "react";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import { controlMotion } from "../control-motion";
import * as s from "./targetOffer.css";

/** The caller supplies the current instruction and optional one-based occurrence. */
export function TargetPrompt({
  instruction,
  step,
}: {
  instruction: string;
  step?: { current: number; total: number };
}) {
  const animate = useAllowsControlMotion();
  return (
    <div role="status" aria-atomic="true" className={s.prompt}>
      {instruction}
      {step && (
        <>
          <span aria-hidden>·</span>
          <motion.span
            key={step.current}
            initial={animate ? { opacity: 0.65 } : false}
            animate={{ opacity: 1 }}
            transition={{ duration: animate ? controlMotion.feedback : 0 }}
            aria-label={`Target ${step.current} of ${step.total}`}
          >
            {step.current}/{step.total}
          </motion.span>
        </>
      )}
    </div>
  );
}
/** Fits the physical card or portrait within a positioned host. The target ID and legality belong to the offer. */
export function TargetOption({
  label,
  eligible,
  onChoose,
  children,
  shape = "card",
}: {
  label: string;
  eligible: boolean;
  onChoose: () => void;
  children: ReactNode;
  shape?: "card" | "portrait";
}) {
  const animate = useAllowsControlMotion();
  return (
    <motion.button
      type="button"
      disabled={!eligible}
      aria-label={label}
      className={s.option}
      data-shape={shape}
      initial={false}
      animate={{
        boxShadow: eligible
          ? "0 0 0 2px #b0fff8, 0 0 4px 2px #18f5ee, 0 0 8px 2px #16a9a6"
          : "0 0 0 0px #b0fff800, 0 0 0px 0px #18f5ee00, 0 0 0px 0px #16a9a600",
      }}
      transition={{ duration: animate ? controlMotion.feedback : 0 }}
      onClick={onChoose}
    >
      {children}
    </motion.button>
  );
}
