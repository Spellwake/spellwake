import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { cssEase, motionLanguage as m } from "../../lib/motion";
import { glassEdge, glassSurface, TOUCH_TARGET, touch } from "./glass.css";

const compact = ':root[data-shell="compact"] &';
/** A phone on its side: the dock moves to the left edge, so content starts beside it. */
const compactLandscape =
  ':root[data-shell="compact"][data-orientation="landscape"] &';
const arrive = cssEase(m.ease.arrive);

/** Shell surfaces sampled from the approved desktop references. */
export const shellTokens = {
  ground: "#070d14",
  ink: "#f6e9d3",
  active: "#ffe8b9",
  line: "#ffecbc",
  glow: "#f2c875",
  headerEdge: "#99857266",
  divider: "#a18b6d44",
  avatarEdge: "#c2aa85",
  headerHeight: "76px",
  compactBar: "84px",
  serif: "Baskerville, Georgia, serif",
} as const;

globalStyle("html, body, #root", { height: "100%", margin: 0 });
globalStyle("body", {
  background: shellTokens.ground,
  color: shellTokens.ink,
  overflow: "hidden",
  overscrollBehavior: "none",
  WebkitFontSmoothing: "antialiased",
  WebkitTapHighlightColor: "transparent",
});
globalStyle("*, *::before, *::after", { boxSizing: "border-box" });

export const scene = style({
  position: "fixed",
  inset: 0,
  isolation: "isolate",
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
  fontFamily: shellTokens.serif,
  fontSize: controlTokens.font.small,
  lineHeight: 1.18,
});
export const workspace = style({
  flex: 1,
  minHeight: 0,
  display: "flex",
  position: "relative",
});
export const stage = style({
  flex: 1,
  minWidth: 0,
  minHeight: 0,
  position: "relative",
  display: "grid",
});
/** Beside the standing dock of a phone on its side. Only frames that show the dock use it. */
export const stageBesideDock = style({
  selectors: {
    [compactLandscape]: {
      paddingLeft: "calc(96px + env(safe-area-inset-left, 0px))",
    },
  },
});
export const page = style({
  gridArea: "1 / 1",
  minWidth: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
});
export const railSlot = style({
  ...glassSurface,
  // No top highlight: it drew a second line under the header rule, across the mark's dip.
  boxShadow: "inset 0 -1px 0 #00000059",
  display: "flex",
  flexShrink: 0,
  borderInlineEnd: `1px solid ${glassEdge}`,
});
/** The rail's sections, which fade in place when the destination changes. */
export const railSections = style({ display: "flex", flex: 1, minWidth: 0 });
// The library rail is sized for a pointer. Touch screens keep each section a fingertip tall.
globalStyle(`${railSlot} a`, {
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});

/* Header ------------------------------------------------------------------ */

export const header = style({
  ...glassSurface,
  // Only the top highlight: the drawn rule below is the edge, and a bottom
  // shadow would run straight across the mark's dip.
  boxShadow: "inset 0 1px 0 #f6e9d314",
  position: "relative",
  zIndex: 2,
  flexShrink: 0,
  minHeight: shellTokens.headerHeight,
  paddingBlock: 0,
  paddingInline:
    "max(30px, env(safe-area-inset-left, 0px)) max(30px, env(safe-area-inset-right, 0px))",
  display: "flex",
  alignItems: "stretch",
  gap: "26px",
  font: `26px/1 ${shellTokens.serif}`,
  selectors: {
    // The bottom rule is drawn rather than bordered so it can break where the mark's notch dips below it.
    "&::after": {
      content: '""',
      position: "absolute",
      insetInline: 0,
      bottom: 0,
      height: "1px",
      // The gap is exactly where the mark's dip takes over the rule.
      background: `linear-gradient(90deg,${shellTokens.headerEdge} 0 34px,transparent 34px 89px,${shellTokens.headerEdge} 89px)`,
      pointerEvents: "none",
    },
    [`${compact}::after`]: { background: shellTokens.headerEdge },
    [compact]: {
      minHeight: shellTokens.compactBar,
      paddingInline:
        "max(22px, env(safe-area-inset-left, 0px)) max(22px, env(safe-area-inset-right, 0px))",
      paddingTop: "env(safe-area-inset-top, 0px)",
      gap: "18px",
    },
  },
});
export const logo = style({
  display: "grid",
  placeItems: "center",
  "@media": { [touch]: { minWidth: TOUCH_TARGET, minHeight: TOUCH_TARGET } },
  paddingRight: "22px",
  position: "relative",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      right: 0,
      top: "22px",
      bottom: "22px",
      width: "1px",
      background: shellTokens.divider,
    },
    // The header rule dips under the mark in a small curve about as wide as
    // the mark itself, as in the approved references.
    "&::before": {
      content: '""',
      position: "absolute",
      left: "4px",
      right: "26px",
      bottom: "-8px",
      height: "9px",
      // The header's own glass, so the dip reads as the header reaching down.
      background: glassSurface.background,
      backdropFilter: glassSurface.backdropFilter,
      WebkitBackdropFilter: glassSurface.WebkitBackdropFilter,
      border: `1px solid ${shellTokens.headerEdge}`,
      borderTop: 0,
      borderRadius: "0 0 50% 50% / 0 0 100% 100%",
    },
    "&:focus-visible": {
      outline: `2px solid ${controlTokens.color.focus}`,
      outlineOffset: "-6px",
    },
    [compact]: { paddingRight: 0 },
    [`${compact}::after`]: { display: "none" },
    [`${compact}::before`]: { display: "none" },
  },
});
const logoBreath = keyframes({
  "0%, 100%": { filter: "drop-shadow(0 0 0 #ffbd5f00)" },
  "50%": { filter: "drop-shadow(0 0 9px #ffbd5f66)" },
});
export const logoMark = style({
  width: "63px",
  height: "51px",
  objectFit: "contain",
  "@media": {
    [standardMotion]: {
      transition: `transform ${m.duration.scene}s ${arrive}`,
      selectors: {
        [`${logo}:hover &`]: {
          transform: "scale(1.06) rotate(-3deg)",
          animation: `${logoBreath} 2.4s ease-in-out infinite`,
        },
      },
    },
  },
});

export const destinationNav = style({
  position: "relative",
  display: "flex",
  alignItems: "stretch",
  gap: "26px",
  marginRight: "auto",
});
export const destinationLink = style({
  position: "relative",
  display: "inline-flex",
  alignItems: "center",
  padding: "0 22px",
  color: shellTokens.ink,
  textDecoration: "none",
  whiteSpace: "nowrap",
  selectors: {
    "&:hover, &[aria-current=page]": { color: shellTokens.active },
    "&:focus-visible": {
      outline: `2px solid ${controlTokens.color.focus}`,
      outlineOffset: "-8px",
    },
  },
  "@media": {
    [touch]: { minHeight: TOUCH_TARGET },
    [standardMotion]: {
      transition: `color ${m.duration.feedback}s ${arrive}, text-shadow ${m.duration.settle}s ${arrive}`,
      selectors: {
        "&:hover": { textShadow: `0 0 18px ${shellTokens.glow}55` },
      },
    },
  },
});
/** The gold line and diamond travel between destinations instead of blinking across. */
export const marker = style({
  position: "absolute",
  left: 0,
  bottom: "-1px",
  height: "2px",
  background: `linear-gradient(90deg,transparent,${shellTokens.line} 22%,${shellTokens.line} 78%,transparent)`,
  pointerEvents: "none",
  selectors: {
    "&::before": {
      content: '""',
      position: "absolute",
      left: "50%",
      top: "50%",
      width: "8px",
      height: "8px",
      background: "#f2d8a9",
      transform: "translate(-50%,-50%) rotate(45deg)",
      boxShadow: `0 0 10px 1px ${shellTokens.glow}99`,
    },
    "&::after": {
      content: '""',
      position: "absolute",
      insetInline: "12%",
      bottom: "1px",
      height: "20px",
      background: `radial-gradient(ellipse 50% 100% at 50% 100%,${shellTokens.glow}3d,transparent 72%)`,
    },
  },
  "@media": {
    // Gradients are dropped in forced colours, so the marker falls back to a system colour.
    "(forced-colors: active)": { background: "Highlight" },
    [standardMotion]: {
      selectors: {
        "&[data-ready=true]": {
          transition: `transform ${m.duration.marker}s ${arrive}, width ${m.duration.marker}s ${arrive}`,
        },
      },
    },
  },
});

export const headerActions = style({
  display: "flex",
  alignItems: "center",
  gap: "26px",
  marginLeft: "auto",
  selectors: { [compact]: { gap: "18px" } },
});
export const balance = style({
  display: "flex",
  alignItems: "center",
  paddingRight: "26px",
  position: "relative",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      right: 0,
      top: "50%",
      height: "30px",
      width: "1px",
      transform: "translateY(-50%)",
      background: shellTokens.divider,
    },
    [compact]: { paddingRight: "18px" },
  },
});
export const iconAction = style({
  position: "relative",
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "48px",
  height: "48px",
  padding: 0,
  border: 0,
  borderRadius: "50%",
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#f6e9d314", color: shellTokens.active },
    "&:focus-visible": {
      outline: `2px solid ${controlTokens.color.focus}`,
      outlineOffset: "2px",
    },
    "&[aria-disabled=true]": { cursor: "default" },
  },
  "@media": {
    [touch]: {
      width: `max(48px, ${TOUCH_TARGET})`,
      height: `max(48px, ${TOUCH_TARGET})`,
    },
    [standardMotion]: {
      transition: `background ${m.duration.feedback}s ${arrive}, color ${m.duration.feedback}s ${arrive}`,
    },
  },
});
export const account = style({
  appearance: "none",
  display: "flex",
  alignItems: "center",
  gap: "12px",
  padding: "0 0 0 26px",
  border: 0,
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
  position: "relative",
  selectors: {
    "&::before": {
      content: '""',
      position: "absolute",
      left: 0,
      top: "50%",
      height: "30px",
      width: "1px",
      transform: "translateY(-50%)",
      background: shellTokens.divider,
    },
    "&:focus-visible": { outline: "none" },
    [compact]: { paddingLeft: "18px" },
  },
  "@media": { [touch]: { minWidth: TOUCH_TARGET, minHeight: TOUCH_TARGET } },
});
/** Neutral portrait until an account supplies its chosen card artwork. */
export const portrait = style({
  position: "relative",
  display: "grid",
  placeItems: "center",
  width: "62px",
  height: "62px",
  borderRadius: "50%",
  border: `1px solid ${shellTokens.avatarEdge}`,
  background: "radial-gradient(circle at 50% 35%,#2a2f38,#11161d 70%)",
  color: "#d9c9a8",
  pointerEvents: "none",
  selectors: {
    [`${account}:hover &, ${account}[aria-expanded=true] &`]: {
      boxShadow: `0 0 0 2px #f6e9d322, 0 0 18px ${shellTokens.glow}66`,
    },
    [`${account}:focus-visible &`]: {
      outline: `2px solid ${controlTokens.color.focus}`,
      outlineOffset: "3px",
    },
  },
  "@media": {
    [standardMotion]: {
      transition: `box-shadow ${m.duration.settle}s ${arrive}`,
    },
  },
});
globalStyle(`${portrait} > svg`, { width: "68%", height: "68%" });
/** The person's chosen picture, clipped to the portrait's circle. */
export const portraitArt = style({
  position: "absolute",
  inset: 0,
  overflow: "hidden",
  borderRadius: "50%",
});
export const chevron = style({
  selectors: {
    [compact]: { display: "none" },
    [`${account}[aria-expanded=true] &`]: { transform: "rotate(180deg)" },
  },
  "@media": {
    [standardMotion]: {
      transition: `transform ${m.duration.settle}s ${arrive}`,
    },
  },
});
/** Something new waits in Messages. */
export const newDot = style({
  position: "absolute",
  top: "8px",
  right: "8px",
  width: "10px",
  height: "10px",
  borderRadius: "50%",
  background: "#f0c873",
  boxShadow: "0 0 0 2px #0d1014",
  pointerEvents: "none",
});
