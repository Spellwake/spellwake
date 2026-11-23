import { useState, type ReactNode } from "react";
import { motion } from "motion/react";
import { controlMotion } from "../control-motion";
import { useAllowsControlMotion } from "../useAllowsControlMotion";
import * as s from "./rank.css";

export type RankIdentityProps = {
  tier: string;
  presentation: "season" | "standings" | "profile";
  ratingLabel?: string;
} & (
  | { emblemSrc: string; emblem?: never }
  | { emblem: ReactNode; emblemSrc?: never }
);

/** Read-only tier identity. Tables with a separate rating column omit ratingLabel. */
export function RankIdentity({
  tier,
  emblem,
  emblemSrc,
  presentation,
  ratingLabel,
}: RankIdentityProps) {
  const animate = useAllowsControlMotion();
  return (
    <div className={s.identity} data-presentation={presentation}>
      <motion.span
        key={tier}
        aria-hidden
        className={s.emblem}
        initial={animate ? { opacity: 0, scale: 0.96 } : false}
        animate={{ opacity: 1, scale: 1 }}
        transition={{
          duration: animate ? controlMotion.settle : 0,
          ease: controlMotion.ease,
        }}
      >
        {emblemSrc !== undefined ? <img src={emblemSrc} alt="" /> : emblem}
      </motion.span>
      <span className={s.copy}>
        <span className={s.tier}>{tier}</span>
        {ratingLabel && <span className={s.rating}>{ratingLabel}</span>}
      </span>
    </div>
  );
}

/** How the bar first fills when its host is itself arriving: after a wait, over a longer beat. */
export interface RankProgressArrival {
  /** Seconds before the fill starts. */
  delay: number;
  /** Seconds the first fill takes. */
  duration: number;
}
export type RankProgressProps =
  | { highest: true; label: string }
  | {
      highest?: false;
      rating: number;
      lowerBound: number;
      nextBound: number;
      label: string;
      /** Absent, the bar fills at the control's own quick beat. Later changes always do. */
      arrival?: RankProgressArrival;
    };

/** Tier bounds and the localized label come from the host's season data. */
export function RankProgress(props: RankProgressProps) {
  const animate = useAllowsControlMotion();
  // The first fill may be slower than later changes; this says it has happened.
  const [filled, setFilled] = useState(false);
  if (props.highest)
    return <span className={s.progressLabel}>{props.label}</span>;
  const { rating, lowerBound, nextBound, label } = props;
  const first = !filled && animate ? props.arrival : undefined;
  const valid =
    Number.isFinite(rating) &&
    Number.isFinite(lowerBound) &&
    Number.isFinite(nextBound) &&
    nextBound > lowerBound;
  if (!valid) return <span className={s.progressLabel}>{label}</span>;
  const value = Math.min(nextBound, Math.max(lowerBound, rating));
  const fraction = (value - lowerBound) / (nextBound - lowerBound);
  return (
    <div className={s.progress}>
      <span
        className={s.track}
        role="meter"
        aria-label="Rank progress"
        aria-valuemin={lowerBound}
        aria-valuemax={nextBound}
        aria-valuenow={value}
        aria-valuetext={label}
      >
        <span className={s.glow}>
          <span className={s.mask}>
            <motion.span
              className={s.fill}
              initial={animate ? { x: "-100%" } : false}
              animate={{ x: `${(fraction - 1) * 100}%` }}
              transition={{
                duration: first
                  ? first.duration
                  : animate
                    ? controlMotion.settle
                    : 0,
                delay: first ? first.delay : 0,
                ease: controlMotion.ease,
              }}
              onAnimationComplete={() => setFilled(true)}
            />
          </span>
        </span>
      </span>
      <span aria-hidden className={s.progressLabel}>
        {label}
      </span>
    </div>
  );
}
