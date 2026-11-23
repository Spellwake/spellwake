import { createVar, globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { glassEdge, glassSurface, TOUCH_TARGET, touch } from "./glass.css";
import { shellTokens } from "./shell.css";

const compact = ':root[data-shell="compact"] &';
const compactPortrait =
  ':root[data-shell="compact"][data-orientation="portrait"] &';
const compactLandscape =
  ':root[data-shell="compact"][data-orientation="landscape"] &';
const safe = (side: "top" | "right" | "bottom" | "left", minimum: string) =>
  `max(${minimum}, env(safe-area-inset-${side}, 0px))`;

/* Focus frame: one task, one header. ------------------------------------- */

export const taskHeader = style({
  ...glassSurface,
  position: "relative",
  zIndex: 2,
  flexShrink: 0,
  minHeight: shellTokens.headerHeight,
  display: "flex",
  alignItems: "center",
  gap: "22px",
  paddingBlock: "10px",
  paddingInline: `${safe("left", "22px")} ${safe("right", "22px")}`,
  borderBottom: `1px solid ${shellTokens.headerEdge}`,
  selectors: {
    [compact]: {
      gap: "12px",
      paddingTop: `calc(10px + env(safe-area-inset-top, 0px))`,
      paddingInline: `${safe("left", "14px")} ${safe("right", "14px")}`,
    },
  },
});
export const taskTitleRow = style({
  flex: 1,
  minWidth: 0,
  display: "flex",
  alignItems: "center",
  gap: "10px",
  paddingLeft: "22px",
  borderLeft: `1px solid ${shellTokens.divider}`,
  selectors: { [compact]: { paddingLeft: "12px" } },
});
const titleType = {
  font: `400 34px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
} as const;
export const taskTitle = style({
  ...titleType,
  minWidth: 0,
  margin: 0,
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
  selectors: { [compact]: { fontSize: "26px" } },
});
/** Editing happens where the title sits, in the title's own type, with no added box. */
export const taskTitleInput = style({
  ...titleType,
  flex: 1,
  minWidth: 0,
  padding: 0,
  border: 0,
  borderBottom: `1px solid ${shellTokens.line}`,
  background: "transparent",
  outline: "none",
  selectors: { [compact]: { fontSize: "26px" } },
});
export const taskRename = style({
  appearance: "none",
  flexShrink: 0,
  display: "grid",
  placeItems: "center",
  width: "40px",
  height: "40px",
  padding: 0,
  border: 0,
  borderRadius: "8px",
  background: "transparent",
  color: shellTokens.ink,
  cursor: "pointer",
  "@media": {
    [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET },
  },
  selectors: {
    "&:hover": { color: shellTokens.active },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
  },
});
export const taskActions = style({
  display: "flex",
  alignItems: "center",
  gap: "14px",
  flexShrink: 0,
  selectors: { [compact]: { gap: "10px" } },
});

/* Immersive frame: the screen is the stage, controls float. --------------- */

/** Height the screen reserves along the bottom edge, such as a hand of cards. */
export const trayHeight = createVar();

export const immersive = style({
  position: "relative",
  flex: 1,
  minWidth: 0,
  minHeight: 0,
  overflow: "hidden",
  isolation: "isolate",
  fontFamily: shellTokens.serif,
});
export const immersiveStage = style({ position: "absolute", inset: 0 });
export const hudLeading = style({
  position: "absolute",
  zIndex: 2,
  top: safe("top", "14px"),
  left: safe("left", "16px"),
  display: "flex",
  gap: "10px",
});
export const hudTrailing = style({
  position: "absolute",
  zIndex: 2,
  top: safe("top", "14px"),
  right: safe("right", "16px"),
  display: "flex",
  gap: "10px",
  selectors: {
    // Beside the turn panel, which takes the corner on a phone held sideways.
    [compactLandscape]: { right: `calc(${safe("right", "16px")} + 336px)` },
  },
});
const rowFade =
  "linear-gradient(90deg,transparent 0,#000 18px,#000 calc(100% - 18px),transparent 100%)";
/** Seats in turn order. The row scrolls sideways when the table has more seats than width. */
export const hudPlayers = style({
  position: "absolute",
  zIndex: 2,
  top: safe("top", "10px"),
  // Clear of the three corner controls: settings, table chat and the table view.
  left: "220px",
  right: "110px",
  display: "flex",
  gap: "14px",
  paddingBlock: "4px",
  paddingInline: "18px",
  overflowX: "auto",
  overscrollBehaviorX: "contain",
  scrollbarWidth: "none",
  WebkitMaskImage: rowFade,
  maskImage: rowFade,
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
    // An upright phone has no room beside the corner controls, so seats take their own row.
    [compactPortrait]: {
      top: `calc(${safe("top", "14px")} + 72px)`,
      left: 0,
      right: 0,
    },
    // On its side, a phone gives the right edge to the turn panel, so seats stop short of it.
    [compactLandscape]: { left: "206px", right: "436px" },
  },
});
/** Turn state and the actions the match offers. */
export const hudSide = style({
  position: "absolute",
  zIndex: 2,
  top: "84px",
  right: safe("right", "16px"),
  width: "404px",
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-end",
  gap: "14px",
  selectors: {
    // Upright, the actions sit above the tray where a thumb reaches them.
    [compactPortrait]: {
      top: "auto",
      left: safe("left", "16px"),
      width: "auto",
      bottom: `calc(${trayHeight} + 14px)`,
      alignItems: "stretch",
    },
    // Starting at the top edge leaves the panel clear of the tray on a short screen.
    [compactLandscape]: { top: safe("top", "14px"), width: "320px" },
  },
});
// Upright, the turn panel spans the screen with the actions beneath it.
globalStyle(
  `:root[data-shell="compact"][data-orientation="portrait"] ${hudSide} > section`,
  { maxWidth: "none" },
);
// Library match controls are sized for a pointer. Touch screens keep every one a fingertip tall and wide.
globalStyle(`${hudPlayers} button, ${hudSide} button`, {
  "@media": {
    [touch]: { minWidth: TOUCH_TARGET, minHeight: TOUCH_TARGET },
  },
});
export const hudTray = style({
  position: "absolute",
  zIndex: 1,
  insetInline: 0,
  bottom: 0,
  height: trayHeight,
  paddingBottom: "env(safe-area-inset-bottom, 0px)",
});
/** Round glass control for the corners of an immersive screen. */
export const hudButton = style({
  ...glassSurface,
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "54px",
  height: "54px",
  padding: 0,
  flexShrink: 0,
  borderRadius: "50%",
  border: `1px solid ${glassEdge}`,
  color: shellTokens.ink,
  cursor: "pointer",
  "@media": {
    [touch]: {
      width: `max(54px, ${TOUCH_TARGET})`,
      height: `max(54px, ${TOUCH_TARGET})`,
    },
    "(forced-colors: active)": { border: "1px solid CanvasText" },
  },
  selectors: {
    "&:hover": { color: shellTokens.active },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
  },
});
