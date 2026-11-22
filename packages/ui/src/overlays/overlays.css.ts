import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { feedbackTransition, standardMotion } from "../control-motion";

const enter = keyframes({
  from: { opacity: 0, transform: "translateY(var(--sw-menu-entry,-3px))" },
  to: { opacity: 1, transform: "translateY(0)" },
});
const exit = keyframes({ from: { opacity: 1 }, to: { opacity: 0 } });
const surface = style({
  isolation: "isolate",
  boxSizing: "border-box",
  position: "relative",
  font: '18px/24px "Iowan Old Style", Georgia, serif',
  color: "#f8edda",
  WebkitFontSmoothing: "antialiased",
  textShadow: "0 1px 1px #090c0e",
});
export const frame = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  overflow: "visible",
  zIndex: -1,
  pointerEvents: "none",
  filter: "drop-shadow(0 3px 5px #0005)",
});
globalStyle(`${frame} > path`, {
  "@media": {
    "(forced-colors: active)": {
      fill: "Canvas",
      stroke: "CanvasText",
      forcedColorAdjust: "none",
    },
  },
});
export const menu = style([
  surface,
  {
    // Above the phone drawer (1000), so a menu opened inside one shows over it.
    zIndex: 1100,
    padding: 8,
    width: "max-content",
    minWidth: "min(160px,calc(100vw - 16px))",
    maxWidth: "calc(100vw - 16px)",
    outline: "none",
    selectors: { "&[data-side=top]": { vars: { "--sw-menu-entry": "3px" } } },
    "@media": {
      [standardMotion]: {
        selectors: {
          "&[data-state=open]": { animation: `${enter} ${feedbackTransition}` },
          "&[data-state=closed]": {
            animation: `${exit} ${feedbackTransition}`,
            pointerEvents: "none",
          },
        },
      },
    },
  },
]);
export const scrollArea = style({
  maxHeight: "calc(var(--radix-dropdown-menu-content-available-height) - 16px)",
  overflowY: "auto",
});
export const label = style({});
export const item = style({
  display: "flex",
  alignItems: "center",
  gap: 12,
  boxSizing: "border-box",
  width: "100%",
  minHeight: 40,
  padding: "8px 12px",
  margin: 0,
  border: 0,
  borderRadius: 5,
  backgroundColor: "transparent",
  color: "inherit",
  font: "inherit",
  textShadow: "inherit",
  textAlign: "start",
  outline: "none",
  cursor: "pointer",
  selectors: {
    "&[aria-current=page]": { backgroundColor: "#f2e3c50c" },
    "&[data-highlighted]": { backgroundColor: "#f2e3c51e" },
    "&[data-disabled]": { opacity: 0.5, cursor: "default" },
  },
  "@media": {
    [standardMotion]: { transition: `background-color ${feedbackTransition}` },
    "(forced-colors: active)": {
      selectors: {
        "&[data-highlighted]": {
          backgroundColor: "Highlight",
          color: "HighlightText",
          forcedColorAdjust: "none",
        },
      },
    },
  },
});
globalStyle(
  `${menu}:not([data-input="pointer"]) ${item}:focus-visible ${label}`,
  {
    textDecoration: "underline",
    textUnderlineOffset: 4,
  },
);
export const danger = style({ color: "#ff9b83" });
export const leading = style({
  width: 24,
  flexShrink: 0,
  display: "grid",
  placeItems: "center",
});
globalStyle(`${leading} > svg`, { width: 22, height: 22, strokeWidth: 1.5 });
export const detail = style({
  display: "block",
  fontSize: 15,
  lineHeight: "20px",
  color: "#d5ccba",
  fontStyle: "italic",
});
export const divider = style({
  height: 1,
  border: 0,
  backgroundColor: "#b6ae9e38",
  margin: "4px 12px",
});
export const tipTrigger = style({
  vars: { "--interest-delay-start": ".25s", "--interest-delay-end": ".12s" },
  selectors: {
    "&:focus-visible": { outline: "2px solid #ffebbb", outlineOffset: 2 },
  },
});
globalStyle(`${tipTrigger}[interestfor]`, {
  interestDelayStart: "var(--interest-delay-start)",
  interestDelayEnd: "var(--interest-delay-end)",
} as Record<string, string>);
export const tip = style({
  WebkitFontSmoothing: "antialiased",
  "@media": {
    [standardMotion]: {
      transition: `display ${feedbackTransition} allow-discrete, overlay ${feedbackTransition} allow-discrete`,
    },
  },
  position: "fixed",
  inset: "auto",
  margin: 0,
  padding: 0,
  border: 0,
  overflow: "visible",
  background: "transparent",
  color: "#fff2df",
  maxWidth: "min(32rem,calc(100vw - 16px))",
  width: "max-content",
});
export const tipBody = style([
  surface,
  { padding: "8px 12px", overflowWrap: "anywhere" },
]);
