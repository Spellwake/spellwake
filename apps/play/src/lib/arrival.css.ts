import { globalStyle, keyframes } from "@vanilla-extract/css";
import { standardMotion } from "@spellwake/ui/motion";
import { cssEase, motionLanguage as m } from "./motion";

/**
 * A group's one arrival, for a list whose items are any element: the list
 * carries `data-arriving` while it arrives (see `useArrival`), and each child
 * rises a short way as it fades in, one beat after the child before. The
 * tenth and later children arrive together, so a long list is at rest as
 * soon as a short one.
 */
const rise = keyframes({
  from: { opacity: 0, translate: `0 ${m.travel / 2}px` },
});
const fade = keyframes({ from: { opacity: 0 } });

const BEATS = 9;
globalStyle("[data-arriving] > *", {
  animation: `${fade} ${m.duration.settle}s linear both`,
  "@media": {
    [standardMotion]: {
      animation: `${rise} ${m.duration.scene}s ${cssEase(m.ease.arrive)} both`,
      animationDelay: `calc(var(--arrival-beat, 0) * ${m.stagger}s)`,
    },
  },
});
for (let beat = 1; beat <= BEATS; beat += 1)
  globalStyle(
    beat < BEATS
      ? `[data-arriving] > :nth-child(${beat + 1})`
      : `[data-arriving] > :nth-child(n + ${beat + 1})`,
    { vars: { "--arrival-beat": String(beat) } },
  );
