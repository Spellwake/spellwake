import { AnimatePresence, motion } from "motion/react";
import { Check, Circle } from "lucide-react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./readiness.css";

const statusText = {
  editing: "Not ready",
  ready: "Ready",
  confirmed: "Confirmed",
} as const;
export type TradeParticipantStatus = keyof typeof statusText;

/** A read-only badge displayed beside the participant's name in their tray. */
export function TradeReadinessBadge({
  status,
}: {
  status: TradeParticipantStatus;
}) {
  const animate = useAllowsControlMotion();
  const checked = status !== "editing";
  const Mark = checked ? Check : Circle;
  return (
    <span className={s.badge} data-status={status}>
      <AnimatePresence initial={false}>
        <motion.span
          key={status}
          className={s.badgeContent}
          initial={animate ? { opacity: 0.45, y: 1 } : false}
          animate={{ opacity: 1, y: 0 }}
          transition={{
            duration: animate ? controlMotion.feedback : 0,
            ease: controlMotion.ease,
          }}
        >
          <Mark
            aria-hidden="true"
            className={s.mark}
            strokeWidth={checked ? 3 : 1.6}
          />
          {statusText[status]}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}

const roomStatusText = {
  waiting: "Waiting",
  ready: "Ready",
  awaitingConfirmation: "Waiting for confirmation",
} as const;
export type TradeRoomStatus = keyof typeof roomStatusText;

/** The room supplies its state, or null while both participants edit.
 * `text` replaces the generic words, so a room can name the other person, as
 * in "Waiting for Mira", or say what is in progress.
 * Keep this component mounted so status changes are announced in the same polite region. */
export function TradeStatusLine({
  status,
  text,
}: {
  status: TradeRoomStatus | null;
  text?: string | null;
}) {
  const animate = useAllowsControlMotion();
  const words = text ?? (status ? roomStatusText[status] : null);
  return (
    <div role="status" aria-atomic="true" className={s.statusLine}>
      <AnimatePresence initial={false}>
        {words && (
          <motion.span
            key={words}
            className={s.sentence}
            initial={animate ? { opacity: 0.45 } : false}
            animate={{ opacity: 1 }}
            transition={{
              duration: animate ? controlMotion.settle : 0,
              ease: controlMotion.ease,
            }}
          >
            <span className={s.words}>{words}</span>
          </motion.span>
        )}
      </AnimatePresence>
    </div>
  );
}
