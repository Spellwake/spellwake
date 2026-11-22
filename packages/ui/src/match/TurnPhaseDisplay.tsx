import { useId } from "react";
import { motion } from "motion/react";
import { Moon, Swords } from "lucide-react";
import { Tooltip } from "../overlays/Tooltip";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./match.css";

function BeginningIcon() {
  return (
    <svg
      aria-hidden
      viewBox="0 0 28 28"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
    >
      <path d="M6.5 17.5a7.5 7.5 0 0 1 15 0M1 18h26M2 22h24M14 3v3M4.8 6.7l2 2M23.2 6.7l-2 2M1.5 12.2l2.5 1M26.5 12.2l-2.5 1" />
    </svg>
  );
}
function MainIcon() {
  return (
    <svg
      viewBox="0 0 28 28"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinejoin="round"
    >
      <path d="m13 5 11 2.8-4.2 17-6.2-1.6" />
      <path d="m3.2 6.7 12-2.6 4.1 17.8-12 2.6z" />
    </svg>
  );
}
const phases = [
  { id: "beginning", name: "Beginning", icon: BeginningIcon },
  { id: "first-main", name: "First main", icon: MainIcon },
  { id: "combat", name: "Combat", icon: Swords },
  { id: "second-main", name: "Second main", icon: MainIcon },
  { id: "end", name: "End", icon: Moon },
] as const;
export type MatchPhase = (typeof phases)[number]["id"];
export interface TurnPhaseDisplayProps {
  turnLabel: string;
  phase: MatchPhase;
  phaseLabel: string;
}

/** Phase icons describe the source's current turn. Only the separate offered action advances play. */
export function TurnPhaseDisplay({
  turnLabel,
  phase,
  phaseLabel,
}: TurnPhaseDisplayProps) {
  const instance = useId();
  const animate = useAllowsControlMotion();
  return (
    <section className={s.panel} aria-label="Turn and phases">
      <div className={s.heading}>
        {turnLabel} <span aria-hidden>·</span> {phaseLabel}
      </div>
      <ol className={s.phases}>
        {phases.map(({ id, name, icon: Icon }) => (
          <li key={id} className={s.phase}>
            <Tooltip
              text={name}
              aria-label={name}
              aria-current={phase === id ? "step" : undefined}
              placement="top"
              clearance={36}
              alignOffset={54}
              className={s.phaseButton}
            >
              {phase === id && (
                <motion.span
                  aria-hidden
                  className={s.marker}
                  {...(animate ? { layoutId: instance + "-phase" } : {})}
                  initial={false}
                  transition={{
                    duration: animate ? controlMotion.settle : 0,
                    ease: controlMotion.ease,
                  }}
                />
              )}
              <Icon aria-hidden strokeWidth={1.45} />
            </Tooltip>
          </li>
        ))}
      </ol>
    </section>
  );
}
