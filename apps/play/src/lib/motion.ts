import { controlMotion } from "@spellwake/ui/motion";

/**
 * Spellwake's motion language: calm and soft. Things arrive on one long
 * deceleration, leave quickly, and never overshoot. Durations are multiples of
 * the control library's feedback beat so screens and controls share a pulse.
 */
export const motionLanguage = {
  ease: {
    /** Entrances and landings. Shared with control feedback. */
    arrive: controlMotion.ease,
    /** Exits. Accelerates away without lingering. */
    depart: [0.6, 0, 0.84, 0.2],
    /** Elements that stay on screen while repositioning. */
    travel: [0.65, 0, 0.35, 1],
  },
  duration: {
    feedback: controlMotion.feedback,
    settle: controlMotion.settle,
    /** Selection markers crossing between destinations. */
    marker: 0.28,
    /** A screen or panel arriving. */
    scene: 0.42,
    /** The one hero move in a sequence. */
    hero: 0.84,
    /** Slow camera moves on the illustrated environment. */
    camera: 2.4,
  },
  /** Delay between grouped arrivals. A group finishes within about 0.6 s. */
  stagger: 0.06,
  /** Distance budget for arrivals, in design pixels. */
  travel: 24,
} as const;

export const cssEase = (curve: readonly number[]): string =>
  `cubic-bezier(${curve.join(",")})`;
