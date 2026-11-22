import { style, globalStyle } from "@vanilla-extract/css";
import { feedbackTransition, standardMotion } from "../control-motion";
const font = '"Albert Sans",system-ui,sans-serif';
const ring = { outline: "2px solid #ffdf9e", outlineOffset: 3 };
export const providers = style({
  containerType: "inline-size",
  display: "grid",
  gap: "var(--provider-gap, 13px)",
  width: "100%",
});
export const provider = style({
  selectors: {
    "&&": {
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      width: "100%",
      height: "clamp(36px,12.6cqw,50px)",
      gap: "clamp(9px,3cqw,13px)",
      padding: "0 8px",
      border: "1px solid #29364488",
      borderBottomColor: "#192432",
      borderRadius: 12,
      background: "linear-gradient(115deg,#1c2a38,#192532 58%,#1a2735)",
      boxShadow: "inset 0 1px 1px #b7c8d010",
      font: `500 clamp(16px,4.25cqw,18px)/1.15 ${font}`,
      color: "#f6f7f8",
      textShadow: "0 1px 2px #03070b",
    },
    "&&:enabled:hover": {
      background: "linear-gradient(115deg,#293949,#202f40)",
    },
    "&&:focus-visible": ring,
    "&&:disabled": { opacity: 0.96, cursor: "default" },
  },
  // A fingertip-sized choice on touch screens. The upper-case unit is a real pixel for hosts that rescale design pixels.
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
});
globalStyle(`${provider} img`, {
  width: "clamp(23px,6.2cqw,25px)",
  height: "clamp(18px,5.7cqw,23px)",
  objectFit: "contain",
});
export const codeForm = style({
  containerType: "inline-size",
  fontFamily: font,
  color: "#f5efdf",
  width: "100%",
  margin: 0,
  WebkitFontSmoothing: "antialiased",
});
export const hidden = style({
  position: "absolute",
  width: 1,
  height: 1,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
});
export const label = style({
  display: "block",
  fontSize: "4.85cqw",
  fontWeight: 500,
  lineHeight: 1.15,
  marginBottom: "2.8cqw",
});
export const inputWrap = style({ position: "relative" });
export const slots = style({
  display: "grid",
  gridTemplateColumns: "repeat(6,minmax(0,1fr))",
  gap: "2.25cqw",
});
export const slot = style({
  position: "relative",
  display: "grid",
  placeItems: "center",
  boxSizing: "border-box",
  height: "14.9cqw",
  border: "2px solid #364351",
  borderRadius: "3.3cqw",
  background: "linear-gradient(120deg,#202b38,#1c2937)",
  boxShadow: "inset 0 .5px 1px #cad9e414",
  color: "#f1f1ee",
  font: "500 7cqw/1 " + font,
  fontVariantNumeric: "tabular-nums",
  textShadow: "0 1px 2px #080c10",
  selectors: {
    "&[data-selected=true]": { background: "#3c4c5a" },
    "&[data-active=true]": {
      borderColor: "#ffe2aa",
      boxShadow: "inset 0 0 0 1px #ffe6bb,0 0 8px #ffcf6a55",
    },
    "&[data-invalid=true][data-active=true]": {
      borderColor: "#ff795f",
      boxShadow: "inset 0 0 0 .5px #ff8978,0 0 9px #ff755f8a",
    },
  },
  "@media": {
    [standardMotion]: {
      transition: `box-shadow ${feedbackTransition},border-color ${feedbackTransition},background-color ${feedbackTransition}`,
    },
  },
});
export const native = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  padding: 0,
  margin: 0,
  border: 0,
  opacity: 0,
  fontSize: 24,
  cursor: "text",
  boxSizing: "border-box",
});
export const caret = style({
  position: "absolute",
  height: "46%",
  width: 2,
  background: "#fff0d6",
});
export const error = style({
  margin: "2.5cqw 1% 0",
  fontSize: "4.9cqw",
  lineHeight: 1.2,
  color: "#ff7767",
});
export const verify = style({
  selectors: {
    "&&": {
      width: "100%",
      marginTop: "8.2cqw",
      height: "13.1cqw",
      minHeight: 0,
      padding: 0,
      border: "1px solid #eddfbb",
      borderRadius: "2.5cqw",
      font: `550 5.2cqw/1 ${font}`,
      color: "#18232b",
      background: "linear-gradient(115deg,#f5e2c2,#ead4b3)",
      boxShadow: "none",
    },
    "&&:disabled": {
      opacity: 1,
      color: "#b4aea6",
      background: "linear-gradient(115deg,#67645e,#67625e)",
      borderColor: "#716d65",
      cursor: "default",
    },
    "&&[data-pending=true]": {
      color: "#24303a",
      background: "#a09583",
      borderColor: "#b3a995",
    },
    "&&:focus-visible": ring,
  },
  // A fingertip-sized action on touch screens. The upper-case unit is a real pixel for hosts that rescale design pixels.
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
});
export const resend = style({
  display: "flex",
  justifyContent: "center",
  alignItems: "baseline",
  gap: ".28em",
  marginTop: "4.6cqw",
  fontSize: "4.5cqw",
  lineHeight: 1.2,
  color: "#f1eadb",
  selectors: { "&:has(button:disabled)": { color: "#aaa8a4" } },
});
globalStyle(`${resend} button`, {
  appearance: "none",
  cursor: "pointer",
  // Tall enough to press: 24 pixels for a pointer, a fingertip on touch screens.
  minHeight: "24PX",
  "@media": { "(pointer: coarse)": { minHeight: "44PX", minWidth: "44PX" } },
  padding: 0,
  border: 0,
  background: "transparent",
  font: "inherit",
  color: "#ffe4a4",
  textDecoration: "underline",
  textUnderlineOffset: 3,
});
globalStyle(`${resend} button:disabled`, {
  color: "inherit",
  textDecoration: "none",
  cursor: "default",
});
globalStyle(`${resend} button:focus-visible`, ring);
// The smaller panels on the approved state boards have more space between digits.
globalStyle(slots, {
  "@container": {
    "(max-width: 425px)": { gap: "3.3cqw", paddingInline: "1%" },
    "(max-width: 360px)": { paddingInline: "4%" },
  },
});
globalStyle(slot, {
  "@container": {
    "(max-width: 425px)": {
      height: "13.5cqw",
      borderRadius: "2.6cqw",
      borderWidth: "1.5px",
      borderColor: "#65717a",
      background: "linear-gradient(130deg,#1a2631,#192632)",
    },
    "(max-width: 360px)": { height: "13cqw", borderColor: "#526171" },
  },
});
globalStyle(`${label} ~ ${verify}`, { marginTop: "4.3cqw", height: "12cqw" });
globalStyle(`${label} ~ ${verify}:disabled`, {
  background: "#454b52",
  borderColor: "#62676c",
});
globalStyle(`${codeForm} ${verify}`, {
  "@container": { "(max-width: 360px)": { marginTop: "6cqw" } },
});

globalStyle(`${codeForm} ${verify}`, {
  "@container": { "(max-width:360px)": { fontSize: 19 } },
});
globalStyle(`${codeForm} ${verify}:disabled`, {
  "@container": {
    "(max-width:360px)": {
      color: "#24303a",
      background: "linear-gradient(125deg,#aa9d89,#a19585)",
      borderColor: "#b3a995",
    },
  },
});
globalStyle(resend, {
  "@container": { "(max-width:360px)": { fontSize: 17 } },
});
// The code field is as tall as its slots, which keep to a fingertip on touch screens.
globalStyle(slot, {
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
