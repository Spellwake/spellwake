import { globalStyle, style } from "@vanilla-extract/css";
import { feedbackTransition, standardMotion } from "../control-motion";

const face = '"Times New Roman",serif';
export const row = style({
  appearance: "none",
  display: "block",
  width: "100%",
  position: "relative",
  boxSizing: "border-box",
  padding: "5px clamp(8px,3.2%,28px) 9px",
  minHeight: 114,
  border: 0,
  background: "transparent",
  textAlign: "start",
  cursor: "pointer",
  isolation: "isolate",
  selectors: {
    "&[data-kind=pack]": { minHeight: 125, paddingBlock: "5px 7px" },
    "&::after": {
      content: '""',
      position: "absolute",
      inset: "auto 1% 0",
      height: 1,
      background:
        "linear-gradient(90deg,transparent,#5c6063 4%,#494d50 96%,transparent)",
    },
    "&:focus-visible": {
      outline: "2px solid #ffe0a1",
      outlineOffset: -3,
      borderRadius: 12,
    },
    "&:hover": { backgroundColor: "#c7be9b0a" },
  },
  "@media": {
    [standardMotion]: { transition: `background-color ${feedbackTransition}` },
  },
});
export const selection = style({
  position: "absolute",
  zIndex: -1,
  inset: "0 0 5px",
  borderRadius: 12,
  pointerEvents: "none",
  border: "2px solid #decea7",
  boxShadow: "inset 0 0 0 2px #292824,0 0 5px #c9ac7424",
  background: "linear-gradient(112deg,#242421,#202222 68%,#1d1f20)",
});
export const summary = style({
  position: "relative",
  display: "flex",
  minWidth: 0,
  width: "100%",
  alignItems: "center",
  gap: "clamp(9px,4.6%,30px)",
  font: `400 21px/1.25 ${face}`,
  color: "#e7e0d2",
  textShadow: "0 1px 2px #050709",
  WebkitFontSmoothing: "antialiased",
  textAlign: "start",
  selectors: {
    "&[data-layout=detail]": {
      alignItems: "flex-start",
      gap: "clamp(12px,5.5%,32px)",
      fontSize: 25,
    },
  },
});
export const art = style({
  flex: "0 1 92px",
  width: 92,
  minWidth: 40,
  maxWidth: "24%",
  display: "grid",
  placeItems: "center",
  height: 98,
  overflow: "hidden",
  selectors: {
    [`${summary}[data-kind=pack][data-layout=inline] &`]: {
      height: "auto",
      maxHeight: 121,
    },
    [`${summary}[data-layout=detail] &`]: {
      height: "auto",
      flexBasis: 200,
      width: 200,
      maxWidth: "38%",
      aspectRatio: "200/280",
    },
  },
});
globalStyle(`${art} > :is(img,svg)`, {
  display: "block",
  maxWidth: "100%",
  maxHeight: "100%",
  objectFit: "contain",
});
export const copy = style({
  display: "flex",
  flex: "1 1 0",
  minWidth: 0,
  flexDirection: "column",
  gap: 6,
  overflowWrap: "anywhere",
  whiteSpace: "pre-wrap",
  selectors: {
    [`${summary}[data-layout=detail] &`]: { gap: 8, paddingBlockStart: 17 },
  },
});
export const title = style({
  fontFamily: "Georgia,serif",
  fontSize: 25,
  color: "#f6ede0",
  lineHeight: 1.15,
  selectors: {
    [`${summary}[data-layout=detail] &`]: { fontSize: 32, marginBottom: 4 },
  },
});
export const price = style({
  alignSelf: "center",
  flex: "0 0 auto",
  fontSize: 30,
  color: "#efe7d9",
});

globalStyle(`${row}[data-kind=pack] ${copy}`, {
  position: "relative",
  top: -8,
});
globalStyle(`${row}[data-kind=pack] ${price}`, {
  position: "relative",
  top: -4,
});
globalStyle(`${price} > span`, { gap: ".46em" });

globalStyle(
  `${summary}[data-kind=pack][data-layout=inline] ${art} > :is(img,svg)`,
  { maxHeight: 121 },
);

globalStyle(`${summary}[data-layout=inspector]`, {
  flexDirection: "column",
  alignItems: "stretch",
  gap: 22,
  fontSize: 25,
});
globalStyle(`${summary}[data-layout=inspector] ${art}`, {
  width: "min(100%, 300px)",
  maxWidth: "100%",
  aspectRatio: "300/418",
  height: "auto",
  flex: "none",
  borderRadius: 12,
});
globalStyle(`${summary}[data-layout=inspector] ${art} img`, {
  width: "100%",
  height: "100%",
  objectFit: "contain",
});
globalStyle(`${summary}[data-layout=inspector] ${title}`, {
  fontSize: 36,
  marginBottom: 3,
});
