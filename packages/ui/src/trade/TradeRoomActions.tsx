import { useLayoutEffect, useRef, type Ref } from "react";
import { motion } from "motion/react";
import { ActionButton } from "../button/ActionButton";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import type { TradeParticipantStatus, TradeRoomStatus } from "./TradeReadiness";
import * as s from "./actions.css";

/** The status below the trays also determines which consent action is available. */
export function tradeRoomStatus(
  own: TradeParticipantStatus,
  partner: TradeParticipantStatus,
): TradeRoomStatus | null {
  if (own === "editing") return null;
  if (own === "confirmed") return "awaitingConfirmation";
  return partner === "editing" ? "waiting" : "ready";
}
export interface TradeRoomActionsProps {
  status: TradeRoomStatus | null;
  canReady: boolean;
  pending?: boolean;
  onReady: () => void;
  onTrade: () => void;
  onCancel: () => void;
  cancelButtonRef?: Ref<HTMLButtonElement>;
}
/** Active-room controls. The host owns the offer revision, participant updates and settlement. */
export function TradeRoomActions({
  status,
  canReady,
  pending = false,
  onReady,
  onTrade,
  onCancel,
  cancelButtonRef = null,
}: TradeRoomActionsProps) {
  const editing = status === null;
  const label = editing
    ? "Ready"
    : status === "awaitingConfirmation"
      ? "Confirmed"
      : "Trade";
  const animate = useAllowsControlMotion();
  const group = useRef<HTMLDivElement>(null);
  const primary = useRef<HTMLButtonElement>(null);
  const activated = useRef(false);
  useLayoutEffect(() => {
    if (activated.current && primary.current?.disabled) group.current?.focus();
    activated.current = false;
  }, [status, pending]);
  return (
    <div
      ref={group}
      tabIndex={-1}
      role="group"
      aria-label="Trade actions"
      className={s.group}
      data-editing={editing || undefined}
    >
      <ActionButton
        ref={primary}
        className={s.primary}
        pending={pending}
        disabled={editing ? !canReady : status !== "ready"}
        onClick={(event) => {
          activated.current = event.currentTarget === document.activeElement;
          if (editing) onReady();
          else onTrade();
        }}
      >
        <motion.span
          key={label}
          initial={animate ? { opacity: 0.65, y: 1 } : false}
          animate={{ opacity: 1, y: 0 }}
          transition={{
            duration: animate ? controlMotion.feedback : 0,
            ease: controlMotion.ease,
          }}
        >
          {label}
        </motion.span>
      </ActionButton>
      <ActionButton
        ref={cancelButtonRef}
        className={s.cancel}
        disabled={pending}
        onClick={onCancel}
        aria-haspopup="dialog"
      >
        Cancel trade
      </ActionButton>
    </div>
  );
}
