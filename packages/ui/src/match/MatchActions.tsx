import { motion } from "motion/react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./match.css";
export interface MatchAction {
  label: string;
  onActivate: () => void;
  disabled?: boolean;
  pending?: boolean;
}
export interface MatchActionsProps {
  primary?: MatchAction;
  secondary?: MatchAction;
  status?: string;
  layout?: "turn" | "payment";
}

/** Render only actions the match offers. A pending command guards both buttons. */
export function MatchActions({
  primary,
  secondary,
  status,
  layout = "turn",
}: MatchActionsProps) {
  const animate = useAllowsControlMotion();
  const busy = Boolean(primary?.pending || secondary?.pending);
  const actions =
    layout === "payment" ? [secondary, primary] : [primary, secondary];
  return (
    <div className={s.actions} data-layout={layout}>
      {actions.map(
        (action, i) =>
          action && (
            <ActionButton
              key={i}
              className={action === primary ? s.primary : s.secondary}
              disabled={Boolean(action.disabled || busy)}
              pending={Boolean(action.pending)}
              pendingIndicator={layout === "payment" ? "dots" : "ring"}
              onClick={action.onActivate}
            >
              <motion.span
                key={action.label}
                initial={animate ? { opacity: 0, y: 2 } : false}
                animate={{ opacity: 1, y: 0 }}
                transition={{
                  duration: animate ? controlMotion.feedback : 0,
                  ease: controlMotion.ease,
                }}
              >
                {action.label}
              </motion.span>
            </ActionButton>
          ),
      )}
      <div role="status" className={s.status}>
        {status && (
          <motion.span
            key={status}
            initial={animate ? { opacity: 0 } : false}
            animate={{ opacity: 1 }}
            transition={{ duration: animate ? controlMotion.settle : 0 }}
          >
            {status}
          </motion.span>
        )}
      </div>
    </div>
  );
}
