import { style, globalStyle, keyframes } from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";
import { standardMotion, feedbackTransition } from "../control-motion";

export const boxLabel = style({
  display: "inline-flex",
  alignItems: "center",
  gap: ".65em",
  minHeight: "44px",
  position: "relative",
  cursor: "pointer",
  fontFamily: t.font.face,
  color: t.color.ink,
  fontSize: t.font.small,
  lineHeight: 1.1,
  WebkitFontSmoothing: "antialiased",
  selectors: { "&:has(input:disabled)": { cursor: "default" } },
  // Design pixels shrink with display density; a finger still needs 44 real ones.
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
export const box = style({
  appearance: "none",
  WebkitAppearance: "none",
  boxSizing: "border-box",
  flexShrink: 0,
  margin: 0,
  display: "grid",
  placeContent: "center",
  width: "1.04em",
  height: "1.04em",
  font: "inherit",
  cursor: "inherit",
  border: `1.5px solid ${t.color.goldEdge}`,
  borderRadius: ".14em",
  backgroundColor: t.color.field,
  color: t.color.ink,
  selectors: {
    "&::before": {
      content: '""',
      width: ".50em",
      height: ".27em",
      boxSizing: "content-box",
      borderLeft: ".115em solid currentColor",
      borderBottom: ".115em solid currentColor",
      transform: "translateY(-.07em) rotate(-45deg) scale(.5)",
      opacity: 0,
    },
    "&:checked": { backgroundColor: t.color.gold, color: t.color.field },
    "&:checked::before": {
      transform: "translateY(-.07em) rotate(-45deg) scale(1)",
      opacity: 1,
    },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "3px",
    },
    "&:disabled": { opacity: 0.58 },
  },
  "@media": {
    [standardMotion]: {
      transition: `background-color ${feedbackTransition},border-color ${feedbackTransition}`,
      selectors: {
        "&::before": {
          transition: `transform ${feedbackTransition},opacity ${feedbackTransition}`,
        },
      },
    },
    "(forced-colors: active)": {
      appearance: "auto",
      WebkitAppearance: "revert",
      selectors: { "&::before": { display: "none" } },
    },
  },
});
export const outlined = style({
  selectors: {
    "&:checked": { backgroundColor: t.color.field, color: t.color.gold },
  },
});
export const chip = style({
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: ".48em",
  boxSizing: "border-box",
  minHeight: "1.84em",
  border: `1px solid ${t.color.goldEdge}`,
  borderRadius: t.preference.pill,
  paddingInline: ".82em",
  whiteSpace: "nowrap",
  fontFamily: t.font.face,
  fontSize: t.font.small,
  lineHeight: 1,
  color: t.color.gold,
  background: t.color.field,
  cursor: "pointer",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
    "&:hover": { backgroundColor: t.color.hover },
  },
  "@media": {
    [standardMotion]: { transition: `background-color ${feedbackTransition}` },
  },
});
export const compact = style({
  fontSize: "18px",
  minHeight: "30px",
  paddingInline: ".52em",
  borderRadius: ".5em",
  color: t.color.ink,
  background: t.color.surface,
  borderColor: t.color.edge,
});
export const outlinedChip = style({
  fontSize: "21px",
  minHeight: "38px",
  paddingInline: ".62em",
  borderRadius: ".45em",
  background: t.color.emphasis,
});
globalStyle(`${chip} svg`, {
  width: ".76em",
  height: ".76em",
  strokeWidth: 2,
  flexShrink: 0,
});
const appear = keyframes({
  from: { opacity: 0, transform: "scale(.94)" },
  to: { opacity: 1, transform: "scale(1)" },
});
export const chipEntry = style({
  "@media": {
    [standardMotion]: { animation: `${appear} ${feedbackTransition}` },
  },
});
