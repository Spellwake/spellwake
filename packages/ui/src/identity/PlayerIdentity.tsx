import type { ReactNode } from "react";
import { Eye } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./identity.css";
import { designPixels } from "../design-pixels";

export interface PlayerIdentityProps {
  name: string;
  portrait?: ReactNode;
  detail?: string;
  trailing?: ReactNode;
  presentation:
    "standings" | "profile" | "request" | "heading" | "seat" | "tray";
  onActivate?: () => void;
  actionLabel?: string;
  pressed?: boolean;
  life?: number;
  takingTurn?: boolean;
  viewingOpponent?: boolean;
}
/** A display identity or one named action covering its portrait and text. The owner supplies the destination. */
export function PlayerIdentity({
  name,
  portrait,
  detail,
  trailing,
  presentation,
  onActivate,
  actionLabel,
  pressed,
  life,
  takingTurn,
  viewingOpponent,
}: PlayerIdentityProps) {
  const animate = useAllowsControlMotion();
  const classes = [s.identity, s.layouts[presentation]].join(" ");
  const transition = {
    duration: animate ? controlMotion.feedback : 0,
    ease: controlMotion.ease,
  };
  const seat = presentation === "seat";
  const content = (
    <>
      {seat ? (
        <span className={s.seatArt}>
          {portrait}
          <AnimatePresence>
            {viewingOpponent && (
              <motion.span
                aria-hidden
                className={s.eye}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={transition}
              >
                <Eye size={designPixels(19)} />
              </motion.span>
            )}
          </AnimatePresence>
        </span>
      ) : (
        portrait
      )}
      <span className={s.copy}>
        <span className={s.name}>
          {name}
          {seat && life !== undefined && ` · ${life}`}
        </span>
        {detail && <span className={s.detail}>{detail}</span>}
      </span>
      {trailing}
      {seat && (
        <AnimatePresence>
          {viewingOpponent && (
            <motion.span
              aria-hidden
              className={s.caption}
              initial={{ opacity: 0, y: animate ? -2 : 0 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              transition={transition}
            >
              Viewing {name}
            </motion.span>
          )}
        </AnimatePresence>
      )}
    </>
  );
  if (!onActivate) return <div className={classes}>{content}</div>;
  return (
    <motion.button
      type="button"
      className={classes}
      data-turn={takingTurn || undefined}
      data-viewed={viewingOpponent || undefined}
      aria-label={actionLabel}
      aria-pressed={pressed}
      onClick={onActivate}
      {...(animate ? { whileTap: { scale: 0.985 } } : {})}
      transition={transition}
    >
      {content}
    </motion.button>
  );
}
