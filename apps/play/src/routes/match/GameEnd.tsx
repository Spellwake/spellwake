import { useEffect, useId, useRef, useState, type ReactElement } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ActionButton, useAllowsControlMotion } from "@spellwake/ui";
import type { GameResult, MatchSeries } from "../../match/match-sources";
import { scoreLine, timeLeft } from "../../match/match-series";
import { SideboardDialog } from "./Sideboard";
import * as s from "./game-end.css";

const TITLE: Record<GameResult["outcome"], string> = {
  won: "You won",
  lost: "You lost",
  draw: "Draw",
  "no-result": "No result",
};

// The table's one curve: everything decelerates into place.
const EASE = [0.22, 1, 0.36, 1] as const;
const LEAVE = [0.7, 0, 0.84, 0] as const;

/** Text a person is typing into keeps the keyboard when the result arrives. */
function typing(): boolean {
  const active = document.activeElement;
  return (
    active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement ||
    (active instanceof HTMLElement && active.isContentEditable)
  );
}

/** The game's place in its match, above the result: "Game 1 · You lead 1–0". */
function seriesLine(series: MatchSeries): string {
  const where =
    series.next.kind === "over" ? "Match over" : `Game ${series.gameNumber}`;
  if (!series.score) return where;
  return series.next.kind === "over"
    ? `${where} · ${series.score.you}–${series.score.them}`
    : `${where} · ${scoreLine(series.score)}`;
}

/** A match of several games reads by how the match came out once it is decided. */
function outcomeOf(
  result: GameResult,
  series: MatchSeries | null,
): GameResult["outcome"] {
  return series?.next.kind === "over" ? series.next.outcome : result.outcome;
}
const titleOf = (result: GameResult, series: MatchSeries | null): string =>
  TITLE[outcomeOf(result, series)];

/** What the next game waits on, counted down where there is a time. */
function NextGame({ series }: { series: MatchSeries }): ReactElement | null {
  const next = series.next;
  const deadlineAt = next.kind === "deciding" ? next.deadlineAt : null;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!deadlineAt) return;
    setNow(Date.now());
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(tick);
  }, [deadlineAt]);
  if (next.kind === "over") return null;
  if (next.kind === "waiting")
    return <p className={s.next}>Waiting for the next game…</p>;
  return (
    <p className={s.next}>
      {next.readyFailed
        ? "That didn’t go through. "
        : next.readyPending
          ? "Starting the next game… "
          : ""}
      {deadlineAt ? (
        <>
          Next game in{" "}
          <span className={s.nextTime}>{timeLeft(deadlineAt, now)}</span>
        </>
      ) : (
        !next.readyPending && "The next game starts when everyone is ready."
      )}
    </p>
  );
}

/** Next game while the match goes on; Leave table once nothing is left to play. */
function EndAction({
  series,
  onLeave,
}: {
  series: MatchSeries | null;
  onLeave: () => void;
}): ReactElement | null {
  const next = series?.next;
  if (!next || next.kind === "over")
    return (
      <ActionButton emphasis="primary" onClick={onLeave}>
        Leave table
      </ActionButton>
    );
  if (next.kind !== "deciding") return null;
  return <Deciding next={next} />;
}

/** Between games: sideboard first, or start the next game with the deck as it is. */
function Deciding({
  next,
}: {
  next: Extract<MatchSeries["next"], { kind: "deciding" }>;
}): ReactElement | null {
  const [sideboarding, setSideboarding] = useState(false);
  return (
    <>
      {next.sideboard && (
        <ActionButton
          disabled={next.readyPending}
          onClick={() => setSideboarding(true)}
        >
          Sideboard
        </ActionButton>
      )}
      {next.ready && (
        <ActionButton
          emphasis="primary"
          pending={next.readyPending}
          onClick={next.ready}
        >
          Next game
        </ActionButton>
      )}
      {sideboarding && next.sideboard && (
        <SideboardDialog
          sideboard={next.sideboard}
          onClose={() => setSideboarding(false)}
        />
      )}
    </>
  );
}

/**
 * The result where the turn's actions were, once the band has been lifted to
 * look at the finished table.
 */
export function GameEndPanel({
  result,
  series,
  onLeave,
}: {
  result: GameResult;
  series: MatchSeries | null;
  onLeave: () => void;
}): ReactElement {
  const panel = useRef<HTMLDivElement>(null);
  // The band that held the keyboard is gone; the result keeps it.
  useEffect(() => {
    if (!typing()) panel.current?.focus({ preventScroll: true });
  }, []);
  return (
    <div
      ref={panel}
      className={s.panel}
      role="group"
      aria-label="Game result"
      data-outcome={outcomeOf(result, series)}
      tabIndex={-1}
    >
      {series && <p className={s.series}>{seriesLine(series)}</p>}
      <p className={s.panelTitle}>{titleOf(result, series)}</p>
      {result.reason && <p className={s.reason}>{result.reason}</p>}
      {series && <NextGame series={series} />}
      <EndAction series={series} onLeave={onLeave} />
    </div>
  );
}

/**
 * The result as a band across the table, which dims behind it. It arrives
 * once, when the game ends, and lifts to leave the finished table in view.
 */
export function GameEndBand({
  open,
  result,
  series,
  onView,
  onLeave,
}: {
  open: boolean;
  result: GameResult;
  series: MatchSeries | null;
  /** Lifts the band and leaves the finished table in view. */
  onView: () => void;
  onLeave: () => void;
}): ReactElement {
  const animate = useAllowsControlMotion();
  const title = useId();
  const reason = useId();
  const band = useRef<HTMLElement>(null);
  // The result is read out, and View table is one Tab away. Focus rests on the
  // band, not on a button, so a key already on its way down leaves nothing.
  useEffect(() => {
    if (open && !typing()) band.current?.focus({ preventScroll: true });
  }, [open]);

  const settle = (delay: number, duration = 0.56) =>
    animate ? { duration, delay, ease: EASE } : { duration: 0 };
  const lift = (duration: number) =>
    animate ? { duration, ease: LEAVE } : { duration: 0 };
  const rise = {
    initial: animate ? { opacity: 0, y: 8 } : false,
    animate: { opacity: 1, y: 0 },
    exit: { opacity: 0, transition: lift(0.14) },
  } as const;

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          key="scrim"
          className={s.scrim}
          aria-hidden="true"
          initial={animate ? { opacity: 0 } : false}
          animate={{ opacity: 1, transition: settle(0, 0.7) }}
          exit={{ opacity: 0, transition: lift(0.3) }}
        />
      )}
      {open && (
        <motion.section
          key="band"
          ref={band}
          className={s.band}
          role="dialog"
          aria-labelledby={title}
          {...(result.reason ? { "aria-describedby": reason } : {})}
          data-outcome={outcomeOf(result, series)}
          tabIndex={-1}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.stopPropagation();
            onView();
          }}
          // The surface closes last, after what it holds has gone.
          exit={{ opacity: 1, transition: lift(0.3) }}
        >
          <motion.span
            className={s.bandSurface}
            aria-hidden="true"
            initial={animate ? { opacity: 0, scaleY: 0 } : false}
            animate={{ opacity: 1, scaleY: 1, transition: settle(0, 0.5) }}
            exit={{
              opacity: 0,
              scaleY: 0,
              transition: animate
                ? { duration: 0.24, delay: 0.06, ease: LEAVE }
                : { duration: 0 },
            }}
          />
          {series && (
            <motion.p
              className={s.series}
              initial={animate ? { opacity: 0 } : false}
              animate={{ opacity: 1, transition: settle(0.16) }}
              exit={{ opacity: 0, transition: lift(0.14) }}
            >
              {seriesLine(series)}
            </motion.p>
          )}
          <motion.h2
            id={title}
            className={s.bandTitle}
            initial={animate ? { opacity: 0, letterSpacing: "0.24em" } : false}
            animate={{
              opacity: 1,
              letterSpacing: "0.04em",
              transition: settle(0.16, 0.84),
            }}
            exit={{ opacity: 0, transition: lift(0.14) }}
          >
            {titleOf(result, series)}
          </motion.h2>
          <motion.span
            className={s.bandRule}
            aria-hidden="true"
            initial={animate ? { opacity: 0, scaleX: 0 } : false}
            animate={{ opacity: 1, scaleX: 1, transition: settle(0.34, 0.7) }}
            exit={{ opacity: 0, transition: lift(0.14) }}
          />
          {result.reason && (
            <motion.p
              id={reason}
              className={s.reason}
              {...rise}
              transition={settle(0.48)}
            >
              {result.reason}
            </motion.p>
          )}
          {series && series.next.kind !== "over" && (
            <motion.div {...rise} transition={settle(0.52)}>
              <NextGame series={series} />
            </motion.div>
          )}
          <motion.div
            className={s.bandActions}
            {...rise}
            transition={settle(0.58)}
          >
            <ActionButton onClick={onView}>View table</ActionButton>
            <EndAction series={series} onLeave={onLeave} />
          </motion.div>
        </motion.section>
      )}
    </AnimatePresence>
  );
}
