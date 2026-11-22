/** Shared timing for control feedback. Motion uses seconds, CSS accepts the same values with a unit. */
export const controlMotion = {
  feedback: 0.12,
  settle: 0.18,
  ease: [0.22, 0.8, 0.24, 1],
} as const;

export const standardMotion = "(prefers-reduced-motion: no-preference)";
export const feedbackTransition = `${controlMotion.feedback}s cubic-bezier(${controlMotion.ease.join(",")})`;
export const settleTransition = `${controlMotion.settle}s cubic-bezier(${controlMotion.ease.join(",")})`;
