import { style, globalStyle } from "@vanilla-extract/css";
import { standardMotion, feedbackTransition } from "../control-motion";
const sans = '"Albert Sans",system-ui,sans-serif',
  serif = 'Baskerville,"Times New Roman",serif';
const focus = { outline: "2px solid #ffe2a1", outlineOffset: -3 };
export const hidden = style({
  position: "absolute",
  width: 1,
  height: 1,
  clipPath: "inset(50%)",
  overflow: "hidden",
  whiteSpace: "nowrap",
});
export const tableMessage = style({
  WebkitFontSmoothing: "antialiased",
  font: `400 var(--chat-text-size,18px)/1.35 ${sans}`,
  color: "#fff3d8",
  textShadow: "0 1px 2px #100e0b",
  overflowWrap: "anywhere",
});
export const author = style({
  color: "#ffda79",
  marginInlineEnd: 6,
  fontWeight: 500,
});
export const body = style({ whiteSpace: "pre-wrap" });
export const directMessage = style({
  display: "flex",
  alignItems: "flex-start",
  width: "100%",
  gap: 12,
  color: "#f5eee3",
  WebkitFontSmoothing: "antialiased",
  font: `400 19px/1.15 ${serif}`,
  textShadow: "0 1px 2px #000",
  selectors: { "&[data-outgoing=true]": { justifyContent: "flex-end" } },
});
export const bubble = style({
  borderRadius: 13,
  background: "linear-gradient(120deg,#2f2e2e,#323132)",
  padding: "8px 16px",
  minWidth: 0,
  maxWidth: "76%",
  whiteSpace: "pre-wrap",
  overflowWrap: "anywhere",
  selectors: {
    [`${directMessage}[data-outgoing=true] &`]: {
      background: "linear-gradient(110deg,#66503a,#705a42)",
    },
  },
});
export const timestamp = style({
  color: "#e8cc91",
  flex: "none",
  alignSelf: "center",
  marginInlineStart: "auto",
  fontSize: 16,
  whiteSpace: "nowrap",
  selectors: {
    [`${directMessage}[data-outgoing=true] &`]: { marginInlineStart: 10 },
  },
});
export const composer = style({
  width: "100%",
  color: "#f6efde",
  font: `400 20px/1.2 ${serif}`,
  selectors: {
    "&[data-surface=table]": {
      WebkitFontSmoothing: "antialiased",
      font: `400 18px/1.2 ${sans}`,
    },
  },
});
export const field = style({
  boxSizing: "border-box",
  display: "flex",
  width: "100%",
  alignItems: "center",
  minHeight: 50,
  margin: 0,
  background: "linear-gradient(115deg,#2d2c2e,#282728)",
  border: "1px solid #484543",
  borderRadius: 12,
  boxShadow: "inset 0 1px 1px #ffffff06",
  selectors: {
    [`${composer}[data-surface=table] &`]: {
      minHeight: 40,
      borderRadius: 19,
      borderColor: "#96866b",
      background: "linear-gradient(120deg,#2a2b29,#222421)",
    },
    "&:focus-within:not([data-blocked=true])": {
      borderColor: "#ffe1ac",
      boxShadow: "inset 0 0 0 1px #ffe1ac,0 0 8px #ecc17744",
    },
  },
  "@media": {
    [standardMotion]: {
      transition: `border-color ${feedbackTransition},box-shadow ${feedbackTransition}`,
    },
  },
});
globalStyle(`${field} textarea`, {
  display: "block",
  boxSizing: "border-box",
  margin: 0,
  minWidth: 0,
  flex: 1,
  resize: "none",
  height: "2.32em",
  padding: ".55em 0 .45em 17px",
  font: "inherit",
  lineHeight: 1.3,
  background: "transparent",
  color: "inherit",
  textShadow: "0 1px 2px #000",
  border: 0,
  outline: 0,
});
globalStyle(`${composer}[data-surface=table] textarea`, {
  padding: ".39em 0 .34em 14px",
  height: "2em",
});
globalStyle(`${field} textarea::placeholder`, { color: "#e4ded4", opacity: 1 });
globalStyle(`${field}[data-blocked=true] textarea`, { color: "#b9b4ad" });
globalStyle(`${field} button`, {
  alignSelf: "stretch",
  width: 55,
  display: "grid",
  placeItems: "center",
  background: "transparent",
  color: "#ffcf74",
  border: 0,
  borderRadius: "inherit",
  padding: 0,
  cursor: "pointer",
});
globalStyle(`${field} button span, ${field} button svg`, { display: "block" });
globalStyle(`${field} button:disabled`, {
  color: "#bcb6a8",
  cursor: "default",
});
globalStyle(`${field} button:focus-visible`, focus);
globalStyle(`${field} button:enabled:active span`, {
  "@media": { [standardMotion]: { transform: "scale(.92)" } },
});
export const blocked = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 10,
  minHeight: 32,
  margin: "0 0 12px",
  borderRadius: 9,
  background: "#323132",
  font: `18px/1.2 ${serif}`,
  color: "#e0dad2",
});
export const readOnly = style({
  WebkitFontSmoothing: "antialiased",
  font: `italic 16px/1.4 ${sans}`,
  color: "#e7ddcf",
  margin: 0,
});
export const limit = style({
  margin: "4px 0 0",
  fontSize: 14,
  color: "#efcbae",
});

export const tableArrow = style({
  display: "block",
  width: 24,
  height: 22,
  background: "linear-gradient(130deg,#ffe3a2,currentColor 75%)",
  clipPath: "polygon(0 0,100% 50%,0 100%,18% 50%)",
});
globalStyle(`${field}[data-blocked=true]`, {
  minHeight: 42,
  background: "linear-gradient(115deg,#363538,#302f30)",
});
globalStyle(`${field}[data-blocked=true] textarea`, {
  height: 40,
  paddingTop: 6,
  paddingBottom: 6,
});
