import type { ReactElement } from "react";
import { motion } from "motion/react";
import { ActionButton, Dialog, useAllowsControlMotion } from "@spellwake/ui";
import type { GameResult } from "../../match/match-sources";
import * as s from "./game-end.css";

const TITLE: Record<GameResult["outcome"], string> = {
  won: "You won",
  lost: "You lost",
  draw: "Draw",
  "no-result": "No result",
};

/** A seat as the result lists it. */
export type Standing = {
  seatId: string;
  name: string;
  /** "20 life", or "Out" for a seat that left the game. */
  note: string;
  won: boolean;
};

/** The result in the turn panel, where the turn's actions were. */
export function GameEndPanel({
  result,
  onLeave,
}: {
  result: GameResult;
  onLeave: () => void;
}): ReactElement {
  return (
    <div className={s.panel} role="status">
      <p className={s.panelTitle}>{TITLE[result.outcome]}</p>
      {result.reason && <p className={s.reason}>{result.reason}</p>}
      <ActionButton emphasis="primary" onClick={onLeave}>
        Leave table
      </ActionButton>
    </div>
  );
}

/** The result as a dialog over the table, with every seat's final standing. */
export function GameEndDialog({
  open,
  result,
  standings,
  onView,
  onLeave,
}: {
  open: boolean;
  result: GameResult;
  standings: readonly Standing[];
  /** Closes the dialog and leaves the finished table in view. */
  onView: () => void;
  onLeave: () => void;
}): ReactElement {
  return (
    <Dialog
      open={open}
      title={TITLE[result.outcome]}
      {...(result.reason ? { subtitle: result.reason } : {})}
      size="compact"
      onClose={onView}
      actions={
        <>
          <ActionButton onClick={onView}>View table</ActionButton>
          <ActionButton emphasis="primary" onClick={onLeave}>
            Leave table
          </ActionButton>
        </>
      }
    >
      <ul className={s.standings}>
        {standings.map((seat) => (
          <li key={seat.seatId} className={s.standing} data-won={seat.won}>
            <span>{seat.name}</span>
            <span className={s.standingNote}>
              {seat.won ? "Winner · " : ""}
              {seat.note}
            </span>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}

/** The result as a band across the table, which dims behind it. */
export function GameEndBand({
  result,
  onView,
  onLeave,
}: {
  result: GameResult;
  /** Lifts the band and leaves the finished table in view. */
  onView: () => void;
  onLeave: () => void;
}): ReactElement {
  const animate = useAllowsControlMotion();
  return (
    <>
      <motion.div
        className={s.scrim}
        aria-hidden="true"
        initial={animate ? { opacity: 0 } : false}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.5, ease: "easeOut" }}
      />
      <motion.section
        className={s.band}
        role="status"
        aria-label="Game result"
        style={{ y: "-50%" }}
        initial={animate ? { opacity: 0, scaleY: 0.2 } : false}
        animate={{ opacity: 1, scaleY: 1 }}
        transition={{ duration: 0.42, ease: [0.2, 0.8, 0.2, 1] }}
      >
        <motion.h2
          className={s.bandTitle}
          initial={animate ? { opacity: 0, letterSpacing: "0.22em" } : false}
          animate={{ opacity: 1, letterSpacing: "0.04em" }}
          transition={{ duration: 0.7, delay: 0.18, ease: [0.2, 0.8, 0.2, 1] }}
        >
          {TITLE[result.outcome]}
        </motion.h2>
        <span className={s.bandRule} aria-hidden="true" />
        {result.reason && <p className={s.reason}>{result.reason}</p>}
        <div className={s.bandActions}>
          <ActionButton onClick={onView}>View table</ActionButton>
          <ActionButton emphasis="primary" onClick={onLeave}>
            Leave table
          </ActionButton>
        </div>
      </motion.section>
    </>
  );
}
