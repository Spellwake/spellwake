import { style } from "@vanilla-extract/css";
import { standardMotion, feedbackTransition } from "../control-motion";
const focus = { outline: "2px solid #fff1bf", outlineOffset: 3 };
export const rail = style({
  height: "100%",
  width: "100%",
  minHeight: 0,
  boxSizing: "border-box",
  display: "flex",
  flexDirection: "column",
  alignItems: "stretch",
  border: "1px solid #4c4438",
  borderInlineEndColor: "#49453e",
  borderRadius: "14px 0 0 14px",
  background: "linear-gradient(105deg,#202020 0%,#242323 54%,#202020)",
});
export const friends = style({
  display: "flex",
  flex: "1 1 auto",
  minHeight: 0,
  flexDirection: "column",
  alignItems: "center",
  gap: 18,
  padding: "47px 6px 8px",
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
});
export const person = style({
  border: 0,
  padding: 0,
  background: "none",
  color: "inherit",
  flex: "none",
  display: "grid",
  placeItems: "center",
  borderRadius: "50%",
  cursor: "pointer",
  selectors: {
    "&:focus-visible": focus,
    "&:hover": { filter: "brightness(1.1)" },
  },
  "@media": {
    [standardMotion]: {
      transition: `filter ${feedbackTransition},transform ${feedbackTransition}`,
      selectors: { "&:active": { transform: "scale(.96)" } },
    },
  },
});
export const utilities = style({
  position: "relative",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "flex-end",
  gap: 18,
  flex: "none",
  minHeight: 164,
  padding: "24px 0 25px",
  boxSizing: "border-box",
  selectors: {
    "&::before": {
      content: '""',
      position: "absolute",
      top: 0,
      width: 38,
      height: 1,
      background: "linear-gradient(90deg,#514a3f,#a8946f,#514a3f)",
    },
  },
});
export const utility = style({
  position: "relative",
  display: "grid",
  placeItems: "center",
  width: 45,
  height: 45,
  padding: 0,
  border: "1px solid #72634c",
  borderRadius: "50%",
  boxShadow: "inset 0 0 0 1px #141414, 0 1px 2px #141414",
  color: "#e6d3a3",
  background: "radial-gradient(ellipse at 50% 20%,#302e2a,#1d1c1b 78%)",
  cursor: "pointer",
  selectors: {
    "&:focus-visible": focus,
    "&:hover": { borderColor: "#d7bf83" },
    "&[aria-current=true]": { color: "#ffdf8f" },
    "&:last-child:not([aria-current])": { color: "#e8e4db" },
  },
  "@media": {
    [standardMotion]: {
      transition: `color ${feedbackTransition},transform ${feedbackTransition}`,
      selectors: { "&:active": { transform: "scale(.96)" } },
    },
  },
});
export const glow = style({
  position: "absolute",
  inset: -1,
  border: "1.5px solid #e8ce83",
  borderRadius: "50%",
  boxShadow: "0 0 8px #f0bc515f,inset 0 0 6px #e6b65c2b",
  pointerEvents: "none",
});
export const badgeAnchor = style({
  position: "absolute",
  insetInlineEnd: -9,
  top: -9,
  display: "grid",
  placeItems: "center",
  pointerEvents: "none",
});
export const count = style({
  boxSizing: "border-box",
  display: "inline-grid",
  placeItems: "center",
  minWidth: 23,
  height: 23,
  paddingInline: 4,
  borderRadius: 20,
  border: "1px solid #f4d2a0",
  color: "#1c1710",
  font: "600 15px/1 system-ui,sans-serif",
  background: "linear-gradient(140deg,#ffd7a0,#e9ab65)",
  boxShadow: "0 0 0 1px #25201c",
});
export const dot = style({
  width: 10,
  height: 10,
  display: "inline-block",
  flex: "none",
  borderRadius: "50%",
  background: "#ffd49d",
  boxShadow: "0 0 8px #cda37290",
});
