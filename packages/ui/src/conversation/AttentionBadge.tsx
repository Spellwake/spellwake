import { AnimatePresence, motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./peopleRail.css";

export type AttentionBadgeProps = { count: number } | { unread: boolean };

/** Visual-only attention indicator. The containing control includes its meaning in its accessible name. */
export function AttentionBadge(props: AttentionBadgeProps) {
  const animate = useAllowsControlMotion();
  const count = "count" in props ? props.count : undefined;
  const visible = "unread" in props ? props.unread : props.count > 0;
  return (
    <AnimatePresence initial={false}>
      {visible && (
        <motion.span
          aria-hidden="true"
          className={count === undefined ? s.dot : s.count}
          key={count === undefined ? "dot" : "count"}
          initial={{ opacity: 0, scale: animate ? 0.85 : 1 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, scale: animate ? 0.85 : 1 }}
          transition={{ duration: animate ? controlMotion.feedback : 0 }}
        >
          {count === undefined ? null : count > 99 ? "99+" : count}
        </motion.span>
      )}
    </AnimatePresence>
  );
}
