import { globalStyle, style } from "@vanilla-extract/css";
import { feedbackTransition, standardMotion } from "../control-motion";
const face = 'Baskerville, "Times New Roman", serif';
const ink = "#fbefd1";
/** Touch screens need a fingertip-sized target. The upper-case unit keeps hosts that rescale design pixels from shrinking it. */
const touch = "(pointer: coarse)";
const TOUCH_TARGET = "44PX";
export const panel = style({
  boxSizing: "border-box",
  width: "100%",
  maxWidth: 405,
  minHeight: 95,
  padding: "7px 17px 8px 21px",
  position: "relative",
  borderRadius: 17,
  border: "1px solid #8b87745c",
  background: "linear-gradient(115deg,rgba(25,30,26,.76),rgba(36,40,39,.81))",
  selectors: {
    "&::before": {
      content: '""',
      position: "absolute",
      inset: -1,
      padding: 1,
      borderRadius: "inherit",
      pointerEvents: "none",
      background:
        "linear-gradient(145deg,#e6d2a8,#83765d 28%,transparent 58%,#7d796958)",
      mask: "linear-gradient(#000 0 0) content-box,linear-gradient(#000 0 0)",
      maskComposite: "exclude",
    },
  },
  color: ink,
  fontFamily: face,
  fontWeight: 400,
  WebkitFontSmoothing: "antialiased",
  textShadow: "0 1px 3px #080b07",
});
export const heading = style({
  padding: "0 4px 6px",
  lineHeight: 1.12,
  fontSize: "1.6rem",
  letterSpacing: ".035em",
  background:
    "linear-gradient(90deg,transparent,#a698774c 8%,#a698774c 86%,transparent) bottom/100% 1px no-repeat",
});
globalStyle(`${heading} span`, { marginInline: 3 });
export const phases = style({
  listStyle: "none",
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  margin: "7px 0 0",
  padding: 0,
});
export const phase = style({
  position: "relative",
  display: "grid",
  placeItems: "center",
  width: 46,
  height: 35,
  "@media": {
    [touch]: {
      width: `max(46px, ${TOUCH_TARGET})`,
      height: `max(35px, ${TOUCH_TARGET})`,
    },
  },
  selectors: {
    "&:not(:last-child)::after": {
      content: '""',
      position: "absolute",
      left: "calc(100% + 5px)",
      width: "clamp(6px, 1.2vw, 17px)",
      height: 1,
      background: "#cfc09b",
      opacity: 0.78,
      pointerEvents: "none",
    },
  },
});
export const phaseButton = style({
  position: "relative",
  isolation: "isolate",
  background: "transparent",
  border: 0,
  width: 41,
  height: 35,
  "@media": {
    [touch]: {
      width: `max(41px, ${TOUCH_TARGET})`,
      height: `max(35px, ${TOUCH_TARGET})`,
    },
  },
  borderRadius: 8,
  display: "grid",
  placeItems: "center",
  padding: 0,
  color: ink,
  selectors: {
    '&[aria-current="step"]': { color: "#352811" },
    "&:hover": { outline: "1px solid #f6e6c7", outlineOffset: 1 },
  },
});
globalStyle(`${phaseButton} svg`, {
  width: 26,
  height: 26,
  filter: "drop-shadow(0 1px 1px #241f1b70)",
});
export const marker = style({
  position: "absolute",
  inset: "1px 0",
  zIndex: -1,
  borderRadius: 8,
  border: "1px solid #ffe4a4",
  background: "linear-gradient(135deg,#ffedb6,#e7bd74 57%,#f9de9f)",
  boxShadow: "inset 0 0 4px #fff5cb,0 0 10px #ecaa4f55",
});
export const actions = style({
  display: "grid",
  justifyItems: "end",
  gap: 12,
  width: "100%",
  fontFamily: face,
});
const action = style({
  fontFamily: face,
  letterSpacing: ".025em",
  lineHeight: 1.1,
  color: ink,
  border: "2px solid #e9cd96",
  textShadow: "0 1px 4px black",
  background: "linear-gradient(115deg,rgba(36,40,35,.83),rgba(35,40,37,.70))",
  boxShadow: "inset 0 1px 12px #e6be6c3d,0 1px 5px #1a211d66",
  selectors: {
    "&:enabled:hover": {
      background: "linear-gradient(115deg,#393e32,#31372f)",
      boxShadow: "inset 0 0 10px #ead6a536,0 0 8px #eacc813d",
    },
    "&:disabled": {
      opacity: 1,
      color: "#aaa698",
      background: "#42443cc7",
      borderColor: "#9e9786",
      boxShadow: "none",
    },
    '&[aria-busy="true"]': {
      color: ink,
      borderColor: "#ddc284",
      background: "rgba(42,53,37,.68)",
    },
  },
  "@media": {
    [standardMotion]: {
      transition: `box-shadow ${feedbackTransition},transform ${feedbackTransition},background-color ${feedbackTransition}`,
    },
  },
});
export const primary = style([
  action,
  {
    width: "100%",
    minHeight: 55,
    height: "auto",
    borderRadius: 18,
    fontSize: "1.95rem",
    padding: "7px 12px",
  },
]);
export const secondary = style([
  action,
  {
    width: "calc(100% - 6px)",
    minHeight: 46,
    height: "auto",
    borderRadius: 18,
    fontSize: "1.5rem",
    borderWidth: 1,
    boxShadow: "0 2px 5px #12160f45",
    selectors: {
      [`${primary}:enabled + &`]: {
        borderColor: "#bcb49a",
        background: "rgba(49,55,47,.77)",
      },
    },
    padding: "5px 10px",
  },
]);
export const status = style({
  paddingTop: 4,
  fontSize: "1.375rem",
  fontStyle: "italic",
  color: ink,
  textShadow: "0 1px 3px black",
  textAlign: "center",
  width: "100%",
  selectors: { "&:empty": { display: "none" } },
});

const payment = `${actions}[data-layout="payment"]`;
globalStyle(payment, { gap: 3 });
globalStyle(`${payment} button`, {
  width: "100%",
  borderRadius: 18,
  border: "2px solid #e9dcbb",
  minHeight: 0,
  boxShadow: "inset 0 1px 4px #e4e3b022,0 3px 5px #13210e80",
  background: "linear-gradient(125deg,#2d3c2b,#354431 53%,#2c3d2b)",
});
globalStyle(`${payment} ${secondary}`, { height: 54, fontSize: 32 });
globalStyle(`${payment} ${primary}`, { height: 49, fontSize: 28 });
// Both payment actions stay fingertip-sized on a touch screen.
globalStyle(`${payment} button`, {
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
globalStyle(`${payment} button:disabled`, {
  opacity: 1,
  borderColor: "#a39e88",
  color: "#b8b29e",
  background: "#3a4439",
  boxShadow: "none",
});
