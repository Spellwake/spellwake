import {
  createThemeContract,
  assignVars,
  globalStyle,
  style,
} from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";
import {
  feedbackTransition,
  settleTransition,
  standardMotion,
} from "../control-motion";
const n = createThemeContract({
  panel: null,
  ink: null,
  active: null,
  edge: null,
  line: null,
  railFill: null,
  tabFill: null,
});
export const theme = style({
  vars: assignVars(n, {
    panel:
      "radial-gradient(ellipse at 14% 65%,#101e282f,transparent 52%),radial-gradient(ellipse at 80% 4%,#14253155,transparent 62%),linear-gradient(108deg,#08131c,#071018 50%,#0b1720)",
    ink: "#fff1d6",
    active: "#ffe8b9",
    edge: "#fbf3d4",
    line: "#ffecbc",
    railFill: "linear-gradient(104deg,#322a1d,#282722 44%,#1b2326 100%)",
    tabFill:
      "radial-gradient(ellipse 57% 17% at 52% 103%,#ffd99b 0%,#ffc166bb 43%,#d4954855 76%,transparent 100%),radial-gradient(ellipse 56% 77% at 52% 108%,#ffc356 0%,#e99a35 24%,#8a5929 42%,#473526 65%,transparent 89%),linear-gradient(180deg,#373124 0%,#292720 24%,#242521 55%,#211f1b)",
  }),
  color: n.ink,
  fontFamily: '"Iowan Old Style", Georgia, serif',
  WebkitFontSmoothing: "antialiased",
  textShadow: "0 1px 1px #080c10",
  WebkitTextStroke: ".14px currentColor",
});
const focus = { outline: `2px solid ${t.color.focus}`, outlineOffset: "-3px" };
export const rail = style({
  background: n.panel,
  borderInlineEnd: "1px solid #3f4545",
  boxSizing: "border-box",
  width: "224px",
  maxWidth: "100%",
  flexShrink: 0,
  padding: "32px 0 0 13px",
  font: "400 23px/1.2 Baskerville, Georgia, serif",
});
/** Lets a host draw its own surface behind the rail, such as glass over artwork. */
export const clearRail = style({
  background: "transparent",
  borderInlineEnd: 0,
});
export const link = style({
  display: "flex",
  position: "relative",
  isolation: "isolate",
  alignItems: "center",
  boxSizing: "border-box",
  gap: "22px",
  minHeight: "66px",
  padding: "8px 8px 8px 24px",
  whiteSpace: "nowrap",
  color: n.ink,
  textDecoration: "none",
  lineHeight: 1.2,
  selectors: {
    "&:focus-visible": focus,
    "&:hover": { color: n.active },
    "&[aria-current=page]": { color: n.active },
  },
});
export const railIcon = style({
  color: "#f2deb0",
  width: "1.25em",
  height: "1.25em",
  display: "grid",
  placeItems: "center",
  flexShrink: 0,
});
globalStyle(`${railIcon} > svg`, {
  width: "100%",
  height: "100%",
  strokeWidth: 1.35,
});
export const railMark = style({
  position: "absolute",
  zIndex: -1,
  inset: "0",
  background: n.railFill,
  borderInlineStart: `2px solid ${n.line}`,
  pointerEvents: "none",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      top: "50%",
      left: "-1px",
      width: "9px",
      height: "9px",
      background: n.line,
      transform: "translate(-50%,-50%) rotate(45deg)",
    },
  },
});
export const tabRoot = style({ minWidth: 0, width: "100%" });
export const viewport = style({
  maxWidth: "100%",
  paddingBottom: "6px",
  overflowX: "auto",
  overflowY: "hidden",
  overscrollBehaviorX: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f #10191e",
});
export const strip = style({
  position: "relative",
  isolation: "isolate",
  display: "flex",
  width: "max-content",
  columnGap: "12px",
  minWidth: "100%",
  alignItems: "stretch",
  borderBottom: "1px solid #495052",
});
export const framed = style({
  background: n.panel,
  columnGap: 0,
  border: `1.5px solid ${n.edge}`,
  borderBottom: 0,
  borderRadius: "24px 24px 0 0",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      pointerEvents: "none",
      top: "calc(100% - 1px)",
      insetInline: "-1px",
      height: "16px",
      background: n.panel,
      borderTop: "2px solid #fbefd2",
      borderRadius: "18px 18px 0 0 / 13px 13px 0 0",
      boxSizing: "content-box",
      clipPath: "inset(0 0 10px)",
    },
  },
});
export const trigger = style({
  position: "relative",
  boxSizing: "border-box",
  appearance: "none",
  cursor: "pointer",
  color: n.ink,
  background: "transparent",
  border: "1px solid transparent",
  padding: "0 16px 18px",
  font: "inherit",
  fontSize: "24.5px",
  minWidth: "135px",
  lineHeight: 1.2,
  whiteSpace: "nowrap",
  textAlign: "center",
  selectors: {
    "&:focus-visible": focus,
    "&:hover, &[aria-selected=true]": {
      color: "#ffe291",
      textShadow: "0 1px 1px #452f13",
    },
    "&[aria-selected=true]": { anchorName: "var(--sw-active-tab)" },
    "&:disabled": { opacity: 0.55, cursor: "default" },
  },
  "@media": {
    [standardMotion]: { transition: `color ${feedbackTransition}` },
    // A fingertip-sized tab on touch screens. The upper-case unit is a real pixel for hosts that rescale design pixels.
    "(pointer: coarse)": { minHeight: "44PX" },
  },
});
export const format = style({
  fontFamily: "Georgia, serif",
  fontSize: "31px",
  flex: "1 0 0px",
  minWidth: "245px",
  borderRadius: "23px 23px 0 0",
  padding: "6px 16px 10px",
  minHeight: "66px",
  selectors: {
    "&:first-child": { flexGrow: 1.11, textIndent: "8px" },
    "&[aria-selected=true]": {
      fontSize: "33px",
      color: "#fff0cc",
      background: n.tabFill,
      borderColor: "#d6a75f",
      borderBlock: "0",
      boxShadow: "inset 0 2px 5px -5px #ffd181",
      textShadow: "0 1px 2px #1a130d",
    },
    "&:first-child[aria-selected=true]": {
      borderInlineStartColor: "transparent",
    },
    "&:nth-last-of-type(1)[aria-selected=true]": {
      borderInlineEndColor: "transparent",
    },
  },
});
export const underline = style({
  position: "absolute",
  pointerEvents: "none",
  height: 0,
  bottom: 0,
  borderBottom: `3px solid ${n.line}`,
  boxShadow: "0 -1px 9px 2px #f2c87568",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      left: "50%",
      width: "10px",
      height: "10px",
      background: n.line,
      transform: "translate(-50%,-35%) rotate(45deg)",
    },
  },
});
export const light = style({
  height: "6px",
  borderBottom: 0,
  bottom: "1px",
  borderRadius: "9px 9px 0 0 / 6px 6px 0 0",
  background:
    "linear-gradient(0deg,#fff4c698,transparent 30%),linear-gradient(90deg,#f8b746,#ffe18b 29%,#ffefb7 43%,#fffbec 53%,#fff1bf 62%,#ffda75 77%,#f6b549)",
  boxShadow: "0 -3px 10px 1px #ffac3c88, 0 0 16px 2px #ee932c70",
  selectors: {
    "&::before": {
      content: '""',
      position: "absolute",
      inset: "-4px 35% 0 40%",
      background:
        "radial-gradient(ellipse at 50% 75%,#fffbedcc 6%,#ffdb9144 43%,transparent 72%)",
    },
    "&::after": {
      width: "23px",
      height: "4px",
      top: "-2.5px",
      left: "53%",
      background: "#fff6d7",
      transform: "translateX(-50%)",
      clipPath: "polygon(0 100%,30% 57%,52% 0,68% 47%,100% 100%)",
    },
  },
  "@media": {
    "(forced-colors: active)": { borderBottom: "3px solid Highlight" },
  },
});
export const anchored = style({
  display: "none",
  "@supports": {
    "(left: anchor(left))": {
      display: "block",
      positionAnchor: "var(--sw-active-tab)",
      left: "calc(anchor(left) + var(--sw-underline-inset, 0px))",
      right: "calc(anchor(right) + var(--sw-underline-inset, 0px))",
    },
  },
  "@media": {
    [standardMotion]: {
      transition: `left ${settleTransition}, right ${settleTransition}`,
    },
  },
});
export const fallback = style({
  left: "var(--sw-underline-inset,0px)",
  right: "var(--sw-underline-inset,0px)",
  "@supports": { "(left: anchor(left))": { display: "none" } },
});
export const panel = style({
  outlineOffset: "-2px",
  selectors: { "&:focus-visible": focus },
});

/** Boxed, equal segments. The current one is filled, with no travelling underline. */
export const segmented = style({
  width: "100%",
  columnGap: 0,
  border: "1px solid #595752",
  borderBottom: "1px solid #595752",
  borderRadius: "8px",
  overflow: "hidden",
});
export const segment = style({
  flex: "1 1 0",
  minWidth: 0,
  padding: "8px 12px",
  fontSize: "22px",
  selectors: {
    "&:not(:first-of-type)": { borderInlineStartColor: "#59575280" },
    "&[aria-selected=true]": {
      color: "#fff0cc",
      background: "linear-gradient(180deg,#3a3a37,#2c2c2a)",
      boxShadow: "inset 0 0 0 1px #8e877680",
      textShadow: "0 1px 2px #1a130d",
    },
    "&:hover": { color: "#ffe291" },
  },
});

/** Separate rounded tabs followed by a rule, as the shared collection picker draws them. */
export const pills = style({
  gap: 13,
  border: 0,
  alignItems: "center",
  selectors: {
    "&::after": {
      content: '""',
      height: 1,
      alignSelf: "end",
      marginBottom: 4,
      background: "#5e5e56",
      flex: 1,
    },
  },
});
export const pill = style({
  selectors: {
    "&&": {
      minWidth: 0,
      width: "min(143px,25vw)",
      height: 42,
      padding: "0 8px",
      justifyContent: "center",
      borderRadius: 10,
      border: "1px solid #867a68",
      color: "#eee7d9",
      font: "23px/1.1 Baskerville,serif",
      background: "linear-gradient(#171b1c,#0b1113)",
    },
    "&&[aria-selected=true]": {
      color: "#fff6dd",
      borderColor: "#d0bfa1",
      background: "linear-gradient(125deg,#4b4336,#352f27)",
    },
  },
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { height: "max(42px, 44PX)" } } },
  },
});
