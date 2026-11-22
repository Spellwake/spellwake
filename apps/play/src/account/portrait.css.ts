import { createVar, style } from "@vanilla-extract/css";
import { standardMotion } from "@spellwake/ui/motion";
import { shellTokens } from "../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../lib/motion";

/** The circle's diameter. The host sets it, so one portrait serves every size. */
export const size = createVar();

export const portrait = style({
  position: "relative",
  flex: "none",
  display: "block",
  width: size,
  height: size,
});
export const clip = style({
  position: "absolute",
  inset: 0,
  display: "grid",
  placeItems: "center",
  overflow: "hidden",
  borderRadius: "50%",
  border: `1.5px solid ${shellTokens.avatarEdge}`,
  background: "radial-gradient(circle at 50% 35%,#2a2f38,#11161d 70%)",
  color: "#d9c9a8",
  boxShadow: "0 0 0 1px #00000066, 0 6px 22px #00000080",
});
/** The stand-in figure shown where no picture is chosen. */
export const blank = style({ width: "62%", height: "62%" });
/**
 * Artwork placed by its crop: the element's own size sets the zoom, and its
 * translation, a fraction of that size, puts the chosen point at the centre.
 */
export const art = style({
  position: "absolute",
  left: "50%",
  top: "50%",
  maxWidth: "none",
  maxHeight: "none",
  userSelect: "none",
  pointerEvents: "none",
  opacity: 0,
  selectors: { '&[data-art-ready="true"]': { opacity: 1 } },
  "@media": {
    [standardMotion]: {
      transition: `opacity ${m.duration.settle}s ${cssEase(m.ease.arrive)}`,
    },
  },
});
export const badge = style({
  position: "absolute",
  right: "1%",
  bottom: "1%",
  display: "grid",
  placeItems: "center",
  width: "25%",
  height: "25%",
  minWidth: "14px",
  minHeight: "14px",
  borderRadius: "50%",
  border: `1.5px solid ${shellTokens.avatarEdge}`,
  background: "radial-gradient(circle at 50% 30%,#3a2d1c,#17120c 75%)",
  color: "#ffe8b9",
  boxShadow: "0 2px 8px #000000a6",
});
export const badgeMark = style({ width: "66%", height: "66%" });
