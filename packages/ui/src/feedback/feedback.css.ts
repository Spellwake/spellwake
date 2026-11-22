import { createThemeContract, assignVars, style } from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";
import { feedbackTransition, standardMotion } from "../control-motion";
const f = createThemeContract({
  secondaryInk: null,
  panelFill: null,
  panelEdge: null,
  titleSize: null,
  copySize: null,
  noticeSize: null,
  surface: null,
  rim: null,
  symbol: null,
});
export const theme = style({
  vars: assignVars(f, {
    secondaryInk: "#e5e0d6",
    panelFill:
      "radial-gradient(ellipse at 33% 0%, #171e26, transparent 75%), linear-gradient(120deg, #10151b, #0d1117)",
    panelEdge: "#716350",
    titleSize: "2.4em",
    copySize: "1.52em",
    noticeSize: "1.15em",
    surface: "linear-gradient(174deg, #1b232b, #11181e 35%, #0e151b)",
    rim: "linear-gradient(165deg, #79766c, #4a4e4e 75%, #62615a)",
    symbol: t.color.ink,
  }),
  fontFamily: t.font.face,
  color: t.color.ink,
});
export const empty = style({
  fontFamily: '"Source Serif 4", Georgia, serif',
  textAlign: "center",
  display: "grid",
  placeItems: "center",
  gap: ".87em",
  maxWidth: "100%",
  overflowWrap: "anywhere",
});
export const panel = style({
  width: "100%",
  minHeight: "14.6em",
  padding: "1.25em",
  border: `1px solid ${f.panelEdge}`,
  borderRadius: t.size.radius,
  background: f.panelFill,
  boxShadow: "inset 0 1px 1px #ffffff09, 0 2px 5px #0005",
  alignContent: "center",
});
export const title = style({
  margin: 0,
  fontSize: f.titleSize,
  fontWeight: 400,
  lineHeight: 1.15,
});
export const description = style({
  color: f.secondaryInk,
  margin: 0,
  fontSize: f.copySize,
  lineHeight: 1.2,
});
export const quiet = style({
  color: f.secondaryInk,
  fontFamily: '"Source Serif 4", Georgia, serif',
  fontStyle: "italic",
  fontSize: "1.65em",
  lineHeight: 1.25,
  margin: 0,
});
export const contained = style({
  margin: 0,
  color: "#e8deca",
  font: "italic 28px/1.22 Georgia,serif",
});
export const emptyAction = style({ marginTop: ".95em" });
export const host = style({ width: "100%", minWidth: 0, position: "relative" });
export const group = style({
  display: "grid",
  justifyItems: "end",
  gap: ".3em",
  width: "100%",
  minWidth: 0,
});
export const floating = style({
  position: "absolute",
  inset: "auto",
  bottom: 0,
  right: 0,
  margin: 0,
  padding: 0,
  border: 0,
  background: "transparent",
  overflow: "visible",
  pointerEvents: "none",
  color: "inherit",
  width: "100%",
  maxWidth: "none",
  selectors: { "&[popover]:not(:popover-open)": { display: "none" } },
  "@supports": {
    "(anchor-name: --notification-anchor)": {
      selectors: {
        "&[popover]": {
          position: "fixed",
          bottom: "anchor(bottom)",
          right: "anchor(right)",
          width: "anchor-size(width)",
          positionVisibility: "anchors-visible",
        },
      },
    },
  },
});
export const message = style({
  margin: 0,
  maxWidth: "100%",
  overflowWrap: "anywhere",
  boxSizing: "border-box",
});
export const toast = style({
  display: "flex",
  alignItems: "center",
  justifySelf: "end",
  gap: ".38em",
  minHeight: "2.25em",
  border: "1px solid",
  borderColor: "transparent",
  borderRadius: ".4em",
  background: `${f.surface} padding-box, ${f.rim} border-box`,
  boxShadow:
    "inset 0 1px 2px #ffffff0d, inset 0 -1px 2px #0004, 0 2px 4px #0007",
  "@media": {
    "(forced-colors: active)": {
      background: "Canvas",
      color: "CanvasText",
      borderColor: "CanvasText",
      boxShadow: "none",
    },
  },
  padding: ".28em .58em",
  color: t.color.ink,
  fontSize: f.noticeSize,
  lineHeight: 1.2,
  pointerEvents: "auto",
});
export const success = style({
  vars: {
    [f.surface]:
      "linear-gradient(177deg, #2f4131, #17251e 23%, #15231d 71%, #1c2b21)",
    [f.rim]:
      "linear-gradient(174deg, #a0a478, #738453 40%, #677d4e 74%, #899a67)",
    [f.symbol]: "#edebb7",
  },
  borderRadius: ".85em",
});
export const warning = style({
  width: "min(100%, 18em)",
  fontSize: "1.07em",
  minHeight: "2.3em",
  vars: {
    [f.surface]:
      "linear-gradient(172deg, #363025, #25241f 21%, #201f1b 72%, #29261f)",
    [f.rim]:
      "linear-gradient(165deg, #cba66d, #ac8a59 48%, #94734c 80%, #bf995f)",
    [f.symbol]: "#e7b962",
  },
  borderRadius: ".85em",
});
export const dismissible = style({ minWidth: "min(100%, 15.5em)" });
export const icon = style({
  color: f.symbol,
  flex: "0 0 auto",
  width: "1.3em",
  height: "1.3em",
  strokeWidth: 1.3,
});
export const close = style({
  display: "grid",
  placeItems: "center",
  marginInlineStart: "auto",
  width: "1.5em",
  height: "1.5em",
  flexShrink: 0,
  padding: 0,
  font: "inherit",
  color: "inherit",
  background: "transparent",
  border: 0,
  borderRadius: ".25em",
  cursor: "pointer",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: 1,
    },
    "&:hover": { background: t.color.emphasis },
  },
  "@media": {
    // A finger needs 44 real pixels; the negative margin keeps the toast's own size.
    "(pointer: coarse)": {
      width: "44PX",
      height: "44PX",
      marginBlock: "-10PX",
      marginInlineEnd: "-10PX",
    },
    [standardMotion]: {
      transition: `transform ${feedbackTransition}`,
      selectors: { "&:active": { transform: "scale(.92)" } },
    },
  },
});
export const inline = style({ justifySelf: "center", textAlign: "center" });
export const srOnly = style({
  position: "absolute",
  width: 1,
  height: 1,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
});
