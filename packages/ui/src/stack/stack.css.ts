import { style, globalStyle } from "@vanilla-extract/css";
import { standardMotion, feedbackTransition } from "../control-motion";
export const pile = style({
  position: "relative",
  isolation: "isolate",
  outline: "none",
});
export const card = style({
  position: "absolute",
  inset: 0,
  borderRadius: 7,
  cursor: "pointer",
  transformOrigin: "50% 50%",
  boxShadow: "0 2px 6px #070b08c9",
  selectors: {
    '&[aria-selected="true"]': {
      outline: "2px solid #ffedac",
      boxShadow: "0 0 7px 2px #ffd87588,0 2px 6px #0c120b",
    },
    // A spell offered as a target carries the same cyan rim as an offered card.
    "&[data-offered]": {
      outline: "2px solid #26e2de",
      boxShadow: "0 0 7px 2px #35dfdb85,0 2px 6px #0c120b",
      cursor: "pointer",
    },
  },
  "@media": {
    [standardMotion]: { transition: `box-shadow ${feedbackTransition}` },
  },
});
globalStyle(
  `${pile}:focus-visible ${card}[aria-selected="true"], ${pile}:focus-visible:not(:has([aria-selected="true"])) ${card}[data-front]`,
  { outline: "2px solid #fff5d7", outlineOffset: 2 },
);
export const surface = style({
  width: "100%",
  height: "100%",
  overflow: "hidden",
  borderRadius: "inherit",
  background: "#15120f",
  pointerEvents: "none",
});
export const caster = style({
  position: "absolute",
  bottom: 8,
  right: 8,
  pointerEvents: "none",
});
export const connection = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  overflow: "visible",
  pointerEvents: "none",
  zIndex: 2147483647,
  filter: "drop-shadow(0 1px 0 rgb(25 29 22 / 58%))",
});
globalStyle(`${card}[data-ability]`, { bottom: "auto", minHeight: "100%" });
globalStyle(`${card}[data-ability] ${surface}`, { height: "auto" });
globalStyle(`${card}[data-ability] ${caster}`, { bottom: -12, right: -5 });

export const ability = style({
  display: "block",
  width: "100%",
});
