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
  position: "relative",
  isolation: "isolate",
  display: "grid",
  minHeight: 224,
  gridTemplateRows:
    "minmax(31px, auto) 0 minmax(18px, auto) minmax(min-content, 1fr) 20px",
  background: "#070707",
  color: "#191714",
  fontFamily: '"Times New Roman", Times, serif',
  selectors: {
    '&[data-illustrated="true"]': {
      gridTemplateRows:
        "minmax(31px, auto) minmax(0, 1fr) minmax(23px, auto) 35px 20px",
    },
  },
});
export const abilityArt = style({
  position: "absolute",
  inset: "6.5% 0 23%",
  zIndex: -1,
  overflow: "hidden",
});
globalStyle(`${abilityArt} img, ${abilityArt} svg`, {
  display: "block",
  width: "100%",
  height: "100%",
  objectFit: "cover",
});
export const frameBand = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  zIndex: -1,
});
const band = style({
  position: "relative",
  isolation: "isolate",
  boxSizing: "border-box",
});
export const abilityName = style([
  band,
  {
    gridRow: 1,
    padding: "8px 15px 4px",
    fontSize: ".93rem",
    lineHeight: 1.1,
    overflowWrap: "anywhere",
  },
]);
export const abilityType = style([
  band,
  {
    gridRow: 3,
    padding: "9px 15px 1px",
    color: "#fffaf2",
    fontSize: ".73rem",
    lineHeight: 1.1,
  },
]);
globalStyle(`${ability}[data-illustrated="false"] ${abilityType}`, {
  paddingTop: 4,
});
export const abilityRules = style([
  band,
  {
    gridRow: 4,
    display: "grid",
    alignItems: "start",
    paddingInline: 16,
    overflowWrap: "anywhere",
    fontSize: ".84rem",
    lineHeight: 1.12,
    whiteSpace: "pre-line",
  },
]);
globalStyle(`${abilityRules} > span`, { paddingBlock: 2 });
globalStyle(`${ability}[data-illustrated="true"] ${abilityRules}`, {
  alignItems: "center",
});
export const abilityFoot = style([band, { gridRow: 5 }]);
