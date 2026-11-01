/** Fade printed faces out before a discrete half-turn, then reveal the new orientation. */
export function createCardFacingFade() {
  let turn = 0;
  let target = 0;
  let opacity = 1;
  let phase: "idle" | "out" | "in" = "idle";
  return {
    sample(desired: number, seconds: number, reduced: boolean) {
      target = desired;
      if (reduced) {
        turn = target;
        opacity = 1;
        phase = "idle";
      } else {
        if (target !== turn && phase !== "out") phase = "out";
        if (target === turn && phase === "out") phase = "in";
        if (phase === "out") {
          opacity = Math.max(0, opacity - seconds / 0.09);
          if (opacity === 0) {
            turn = target;
            phase = "in";
          }
        } else if (phase === "in") {
          opacity = Math.min(1, opacity + seconds / 0.13);
          if (opacity === 1) phase = "idle";
        }
      }
      return {
        turn,
        opacity: opacity * opacity * (3 - 2 * opacity),
        moving: phase !== "idle",
      };
    },
  };
}
