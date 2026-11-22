import {
  createGlobalTheme,
  globalStyle,
  keyframes,
  style,
} from "@vanilla-extract/css";

import {
  standardMotion,
  feedbackTransition,
  settleTransition,
} from "./control-motion";
export const controlTokens = createGlobalTheme(":root", {
  color: {
    ink: "#f4eee4",
    muted: "#c3c0bc",
    hint: "#b2b1ae",
    gold: "#f3e2b7",
    edge: "#746e64",
    fieldEdge: "#595752",
    formEdge: "#948377",
    goldEdge: "#e4d0a0",
    field: "#0d1117",
    surface: "#15181e",
    emphasis: "#2b2926",
    menu: "#12151b",
    highlight: "#302d29",
    hover: "#24272b",
    focus: "#f5e9b9",
    headerEdge: "#9f9279",
    headerFill: "#22272d",
    headerField: "#101821",
    primaryFill: "linear-gradient(100deg,#181a1d,#20201d)",
  },
  font: {
    face: 'Baskerville, "Times New Roman", serif',
    size: "25px",
    small: "23px",
    hint: "20px",
  },
  size: {
    control: "52px",
    compact: "50px",
    compactAction: "44px",
    radius: "10px",
    icon: "24px",
    row: "45px",
  },
  space: { xs: "4px", sm: "8px", md: "16px", lg: "20px", xl: "24px" },
  preference: {
    fill: "#fff0ce",
    rail: "#625f54",
    cropRail: "#363533",
    thumb: "linear-gradient(#fff2d9, #ffd294)",
    thumbEdge: "#f6e4c6",
    on: "#cba977",
    off: "#494740",
    offThumb: "#bcb2a5",
    offEdge: "#666158",
    cropEdge: "#806e54",
    cropSurface: "#0a1013",
    optionInk: "#f6e5c6",
    thumbHalo: "0 0 6px #f4d19833",
    railHeight: "8px",
    railThumb: "20px",
    cropHeight: "6px",
    cropThumb: "17px",
    switchWidth: "64px",
    switchHeight: "33px",
    switchThumb: "24px",
    switchInset: "3px",
    radioSize: "26px",
    radioDot: "13px",
    optionGap: "25px",
    radioGap: "17px",
    adjustButton: "35px",
    adjustRadius: "8px",
    adjustGap: "16px",
    sliderHit: "36px",
    pill: "999px",
    thin: "1px",
    focusGap: "3px",
  },
  ring: { line: "2px", offset: "2px", glow: "0 0 7px #e4d4a360" },
  /** Applied behind control surfaces. A host that makes surfaces translucent sets a blur here. */
  effect: { backdrop: "none" },
});
const t = controlTokens;
const focus = {
  outline: `${t.ring.line} solid ${t.color.focus}`,
  outlineOffset: t.ring.offset,
};
const base = style({
  boxSizing: "border-box",
  minWidth: 0,
  height: t.size.control,
  WebkitFontSmoothing: "antialiased",
  fontFamily: t.font.face,
  fontSize: t.font.size,
  fontWeight: 400,
  lineHeight: 1.1,
  background: t.color.surface,
  backdropFilter: t.effect.backdrop,
  WebkitBackdropFilter: t.effect.backdrop,
  color: t.color.ink,
  border: `1px solid ${t.color.edge}`,
  borderRadius: t.size.radius,
});
export const button = style([
  base,
  {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: t.space.md,
    paddingInline: t.space.lg,
    cursor: "pointer",
    "@media": {
      [standardMotion]: {
        transition: `transform ${feedbackTransition}, background-color ${feedbackTransition}, opacity ${feedbackTransition}`,
        selectors: { "&:enabled:active": { transform: "scale(.98)" } },
      },
    },
    selectors: {
      "&:enabled:hover": { background: t.color.hover },
      "&:enabled:active": { background: t.color.menu },
      "&:focus-visible": focus,
      "&:disabled": { opacity: 0.58, cursor: "default" },
    },
  },
]);
export const emphasis = style({
  color: t.color.gold,
  background: t.color.emphasis,
  borderColor: t.color.goldEdge,
  boxShadow: "inset 0 0 0 .5px currentColor",
});
export const headerButton = style({
  paddingInline: t.space.md,
  gap: t.space.sm,
  height: t.size.compact,
  fontSize: t.font.small,
  background: t.color.headerFill,
  borderColor: t.color.goldEdge,
});
export const formButton = style({ borderColor: t.color.formEdge });
export const formPrimary = style({
  background: t.color.primaryFill,
  borderColor: "#fff3d6",
  color: "#fff3d6",
});
export const compactFormButton = style({
  height: t.size.compactAction,
  fontSize: t.font.small,
});
export const compactField = style({ height: t.size.compact });
export const icon = style({
  display: "inline-flex",
  flexShrink: 0,
  alignItems: "center",
  justifyContent: "center",
});
globalStyle(`${icon} svg`, {
  width: t.size.icon,
  height: t.size.icon,
  strokeWidth: 1.7,
});
const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  "@media": {
    "(prefers-reduced-motion: no-preference)": {
      animation: `${spin} 1s linear infinite`,
    },
  },
});
export const field = style({
  minWidth: 0,
  display: "grid",
  gap: t.space.sm,
  fontFamily: t.font.face,
  color: t.color.ink,
});
export const label = style({ fontSize: t.font.size, lineHeight: 1.25 });
export const hidden = style({
  position: "absolute",
  width: 1,
  height: 1,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
});
export const frame = style([
  base,
  {
    display: "flex",
    alignItems: "center",
    gap: t.space.md,
    paddingInline: t.space.md,
    background: t.color.field,
    borderColor: t.color.formEdge,
    color: t.color.muted,
    boxShadow: "inset 0 0 0 .35px currentColor",
    "@media": {
      [standardMotion]: {
        transition: `box-shadow ${feedbackTransition}, border-color ${feedbackTransition}`,
      },
    },
    selectors: {
      "&:focus-within": {
        borderColor: t.color.focus,
        boxShadow: t.ring.glow,
        outline: `1px solid ${t.color.focus}`,
      },
      '&[data-disabled="true"]': { opacity: 0.58 },
    },
  },
]);
export const searchFrame = style({
  paddingInline: t.space.lg,
  gap: "12px",
  borderColor: t.color.fieldEdge,
  boxShadow: "none",
});
export const headerFrame = style({
  height: t.size.compact,
  background: t.color.headerField,
  fontSize: t.font.small,
  gap: t.space.md,
  color: t.color.gold,
});
export const input = style({
  minWidth: 0,
  flex: 1,
  width: "100%",
  height: "100%",
  padding: 0,
  border: 0,
  background: "transparent",
  outline: "none",
  color: t.color.ink,
  font: "inherit",
  selectors: {
    "&::placeholder": { color: "inherit", opacity: 0.78 },
    "&::-webkit-search-cancel-button": { WebkitAppearance: "none" },
  },
});
export const clear = style({
  display: "grid",
  placeItems: "center",
  width: 32,
  height: 32,
  flexShrink: 0,
  padding: 0,
  background: "transparent",
  color: t.color.muted,
  border: 0,
  cursor: "pointer",
  selectors: { "&:focus-visible": focus, "&:hover": { color: t.color.ink } },
  // A fingertip's target on touch screens, overlapping the field's padding so the field keeps its height.
  "@media": {
    "(pointer: coarse)": {
      width: "44PX",
      height: "44PX",
      marginBlock: "-8PX",
      marginInline: "-6PX",
    },
  },
});
export const hint = style({
  fontSize: t.font.hint,
  lineHeight: 1.25,
  color: t.color.hint,
});
export const errorText = style({ color: "#f3d599" });
export const trigger = style([
  base,
  {
    width: "100%",
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: t.space.sm,
    paddingInline: t.space.xl,
    cursor: "pointer",
    textAlign: "left",
    borderColor: t.color.fieldEdge,
    selectors: {
      "&:focus-visible": focus,
      '&[data-state="open"]': { borderColor: t.color.goldEdge },
      "&:disabled": { opacity: 0.58, cursor: "default" },
    },
  },
]);
export const triggerLabel = style({
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const caret = style({ color: t.color.gold, display: "inline-flex" });
const revealMenu = keyframes({
  from: { opacity: 0, transform: "scale(.985)" },
  to: { opacity: 1, transform: "scale(1)" },
});
export const popup = style({
  transformOrigin: "var(--radix-select-content-transform-origin)",
  "@media": {
    [standardMotion]: {
      selectors: {
        '&[data-state="open"]': {
          animation: `${revealMenu} ${settleTransition}`,
        },
      },
    },
  },
  // Above the phone drawer (1000), so a list opened inside one shows over it.
  zIndex: 1100,
  boxSizing: "border-box",
  width: "var(--radix-select-trigger-width)",
  maxHeight: "var(--radix-select-content-available-height)",
  overflow: "hidden",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.headerEdge}`,
  WebkitFontSmoothing: "antialiased",
  background: t.color.menu,
  backdropFilter: t.effect.backdrop,
  WebkitBackdropFilter: t.effect.backdrop,
  color: t.color.ink,
  fontFamily: t.font.face,
  fontSize: t.font.size,
  boxShadow: "0 5px 16px #0007",
});
export const viewport = style({ paddingBlock: t.space.xs });
export const option = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  minHeight: t.size.row,
  paddingInline: t.space.xl,
  gap: t.space.sm,
  outline: "none",
  cursor: "pointer",
  selectors: {
    '&[data-state="checked"]': {
      background: t.color.emphasis,
      color: t.color.gold,
    },
    "&[data-highlighted]": { background: t.color.highlight },
    "&[data-disabled]": { opacity: 0.5, cursor: "default" },
  },
});
