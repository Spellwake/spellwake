import { createVar, keyframes, style } from "@vanilla-extract/css";
import { standardMotion } from "@spellwake/ui/motion";
import { cssEase, motionLanguage as m } from "../../lib/motion";
import { shellTokens } from "./shell.css";

/** How strongly the scrim covers the artwork, from 0 to 1. Set from the artwork's brightness. */
export const scrim = createVar();
/** The artwork's focal point, as CSS percentages. */
export const focusX = createVar();
export const focusY = createVar();

/**
 * The plain ground. It carries the frame on its own until artwork is supplied,
 * and shows through while artwork loads. It stays still: the glass surfaces
 * above re-blur whatever moves beneath them, so nothing here repaints per frame.
 */
export const root = style({
  position: "absolute",
  inset: 0,
  zIndex: -1,
  overflow: "hidden",
  pointerEvents: "none",
  backgroundColor: shellTokens.ground,
  backgroundImage:
    "radial-gradient(ellipse 60% 55% at 78% 18%,#27345c59,transparent 70%),radial-gradient(ellipse 55% 50% at 16% 82%,#5a3a1c40,transparent 72%),linear-gradient(160deg,#0a111b,#070c13 55%,#0b0f18)",
});
const push = keyframes({
  from: { opacity: 0, scale: "1.14", filter: "blur(10px)" },
  to: { opacity: 1, scale: "1.06", filter: "blur(0)" },
});
/**
 * Artwork fills the frame and is cropped around its focal point. It sits a
 * little oversized so the pointer lean never exposes an edge, and arrives with
 * one slow push-in. Nothing else moves the camera.
 */
export const art = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  objectFit: "cover",
  objectPosition: `${focusX} ${focusY}`,
  transformOrigin: `${focusX} ${focusY}`,
  scale: "1.06",
  opacity: 0,
  userSelect: "none",
  willChange: "transform",
  selectors: { "&[data-ready=true]": { opacity: 1 } },
  "@media": {
    // Forced colours drop the scrim, so artwork would sit at full brightness behind text.
    "(forced-colors: active)": { display: "none" },
    [standardMotion]: {
      selectors: {
        "&[data-ready=true]": {
          animation: `${push} ${m.duration.camera}s ${cssEase(m.ease.arrive)} both`,
        },
      },
    },
  },
});
export const motes = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  mixBlendMode: "screen",
  "@media": { "(forced-colors: active)": { display: "none" } },
});
/**
 * Keeps controls legible over any artwork. The flat layer darkens bright art
 * toward the ground colour; the gradients add the header shade and a vignette.
 */
export const veil = style({
  position: "absolute",
  inset: 0,
  background: `linear-gradient(180deg,#05090fbf 0%,#05090f38 26%,#05090f1f 62%,#05090f8c 100%),radial-gradient(ellipse 80% 70% at 50% 46%,transparent 40%,#04070c99 100%),color-mix(in srgb,${shellTokens.ground} calc(${scrim} * 100%),transparent)`,
  "@media": {
    [standardMotion]: {
      transition: `background ${m.duration.hero}s ${cssEase(m.ease.arrive)}`,
    },
  },
});
