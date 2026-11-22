import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";
import { feedbackTransition, standardMotion } from "../control-motion";

const focus = { outline: `2px solid ${t.color.focus}`, outlineOffset: "2px" };
export const group = style({
  display: "inline-flex",
  alignItems: "stretch",
  flexShrink: 0,
  gap: "0.5em",
  fontFamily: t.font.face,
  fontSize: "30px",
  lineHeight: 1,
  color: t.color.ink,
});
export const cell = style({
  display: "inline-grid",
  placeItems: "center",
  boxSizing: "border-box",
  padding: 0,
  margin: 0,
  height: "1.55em",
  border: "1px solid #c8bc9b",
  borderRadius: "0.32em",
  background: "#100f0d",
  color: "inherit",
  font: "inherit",
  textAlign: "center",
  fontVariantNumeric: "lining-nums tabular-nums",
  selectors: {
    "&:focus-visible": focus,
    "&:disabled": { opacity: 0.45 },
    '&[aria-invalid="true"]': { borderColor: "#ec9286" },
  },
});
export const button = style({
  width: "1.72em",
  cursor: "pointer",
  selectors: {
    "&:enabled:hover": { backgroundColor: t.color.emphasis },
    "&:disabled": { cursor: "default" },
  },
  "@media": {
    [standardMotion]: {
      transition: `transform ${feedbackTransition}, background-color ${feedbackTransition}`,
      selectors: { "&:active:enabled": { transform: "scale(.93)" } },
    },
  },
});
export const input = style({ display: "block", placeItems: "normal" });
export const increment = style({
  selectors: {
    "&:enabled": {
      borderColor: "#f3ebd2",
      boxShadow: "inset 0 0 0 .5px #f3ebd2",
    },
  },
});
export const center = style({ width: "2.55em", minWidth: 0 });
export const joined = style({
  gap: 0,
  border: "1px solid #857d69",
  borderRadius: "0.4em",
  background: "#0b1218",
});
export const joinedCell = style({
  background: "transparent",
  border: 0,
  height: "1.85em",
  width: "2.8em",
});
export const joinedValue = style({
  width: "5.1em",
  textAlign: "left",
  paddingInline: ".65em",
  borderInline: "1px solid #857d69",
  borderRadius: ".14em",
});
export const pill = style({
  fontSize: "24px",
  gap: 0,
  background: "linear-gradient(#2f3522,#222919)",
  border: "2px solid #e4dcb3",
  borderRadius: "999px",
  boxShadow: "0 2px 6px #0004",
  color: "#fcf7dd",
});
export const pillCell = style({
  background: "transparent",
  border: 0,
  borderRadius: "999px",
  height: "1.35em",
  width: "1.45em",
});
export const glyph = style({
  width: ".88em",
  height: ".88em",
  pointerEvents: "none",
});
export const incrementGlyph = style({ width: "1.07em", height: "1.07em" });
export const coin = style({
  display: "inline-flex",
  gap: ".32em",
  alignItems: "center",
  verticalAlign: "baseline",
  whiteSpace: "nowrap",
  fontVariantNumeric: "lining-nums tabular-nums",
});
export const icon = style({
  width: "1em",
  height: "1em",
  objectFit: "contain",
  display: "block",
});
export const srOnly = style({
  position: "absolute",
  width: 1,
  height: 1,
  padding: 0,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
});

export const ivory = style({
  selectors: {
    "&&": {
      boxSizing: "border-box",
      justifyContent: "space-evenly",
      alignItems: "center",
      width: 149,
      height: 40,
      font: '500 32px/1 Baskerville,"Times New Roman",serif',
      color: "#19150e",
      border: "1px solid #d2b88c",
      background: "linear-gradient(#fff4df,#e7d4b2 58%,#d5ba8e)",
      boxShadow: "inset 0 1px 1px #fff9,0 2px 3px #17200c70",
    },
  },
});
globalStyle(`${ivory} ${cell}`, {
  boxSizing: "border-box",
  height: 36,
  width: 44,
});
globalStyle(`${ivory} ${glyph}`, { width: 24, height: 24, strokeWidth: 2 });
globalStyle(`${ivory} ${button}:enabled:hover`, {
  backgroundColor: "#ad885b39",
});
globalStyle(`${ivory} ${button}:focus-visible`, {
  outline: "2px solid #4a381e",
  outlineOffset: -3,
});

export const trade = style({
  fontSize: 24,
  background: "linear-gradient(#242526,#101516 65%,#181a18)",
  borderRadius: 9,
  borderColor: "#b5a38d",
});
globalStyle(`${trade} ${joinedCell}`, { height: 38, width: 40 });
globalStyle(`${trade} ${joinedValue}`, {
  width: 70,
  borderInline: "1px solid #252625",
  background: "linear-gradient(#111416,#0e1215)",
  padding: 0,
  textAlign: "center",
});
// A fingertip needs 44 pixels; the trade amount keeps its look and grows to that on touch screens.
globalStyle(`${trade} ${joinedCell}`, {
  "@media": {
    "(pointer: coarse)": {
      height: "max(38px, 44PX)",
      width: "max(40px, 44PX)",
    },
  },
});
// The joined stepper grows to a fingertip on touch screens, as the trade stepper does.
globalStyle(`${joined} ${joinedCell}`, {
  "@media": {
    "(pointer: coarse)": { height: "max(1.85em, 44PX)", minWidth: "44PX" },
  },
});
