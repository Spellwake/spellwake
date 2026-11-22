import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";
import { settleTransition, standardMotion } from "../control-motion";

export const dialog = style({
  boxSizing: "border-box",
  width: "660px",
  maxWidth: "calc(100% - 24px)",
  maxHeight: "calc(100% - 24px)",
  margin: "auto",
  padding: 0,
  overflow: "hidden",
  border: "1.5px solid #e9dcc0",
  borderRadius: "20px",
  background: "linear-gradient(125deg,#0d141b,#0a1016)",
  color: t.color.ink,
  boxShadow: "0 22px 70px #000000b3, inset 0 0 0 1px #ffffff09",
  fontFamily: t.font.face,
  WebkitFontSmoothing: "antialiased",
  selectors: {
    "&[open]": { display: "flex", flexDirection: "column" },
    "&[data-size=compact]": { width: "540px" },
    "&[data-size=wide]": { width: "1210px" },
    "&::backdrop": { background: "#05080cb8" },
  },
  "@media": {
    [standardMotion]: {
      transition: `opacity ${settleTransition}, translate ${settleTransition}`,
      "@starting-style": { opacity: 0, translate: "0 8px" },
    },
  },
});
export const head = style({
  flexShrink: 0,
  display: "flex",
  alignItems: "flex-start",
  gap: "16px",
  margin: "0 22px",
  padding: "26px 10px 20px",
  borderBottom: "1px solid #a698774c",
  selectors: {
    // The close button leads: it is first to the eye, and still last to the keyboard.
    '&[data-close-side="start"]': { flexDirection: "row-reverse" },
    // No title bar: the heading names the dialog without being drawn.
    '[data-chrome="bare"] > &': {
      position: "absolute",
      zIndex: 1,
      insetBlockStart: "14px",
      insetInlineEnd: "14px",
      margin: 0,
      padding: 0,
      border: 0,
    },
  },
});
export const title = style({
  margin: 0,
  font: "inherit",
  fontSize: "36px",
  lineHeight: 1.15,
  // A dialog can start focus here. A heading is not a control, so it draws no ring.
  outline: "none",
  selectors: {
    '[data-chrome="bare"] &': {
      position: "absolute",
      width: "1px",
      height: "1px",
      overflow: "hidden",
      clipPath: "inset(50%)",
      whiteSpace: "nowrap",
    },
  },
});
export const subtitle = style({
  margin: "4px 0 0",
  fontSize: "24px",
  color: t.color.muted,
});
export const close = style({
  appearance: "none",
  flexShrink: 0,
  marginLeft: "auto",
  display: "grid",
  placeItems: "center",
  width: "44px",
  height: "44px",
  padding: 0,
  border: 0,
  borderRadius: "10px",
  // A soft filled plate, so the button reads as a button without an outline.
  background: "#f6e9d314",
  color: "inherit",
  cursor: "pointer",
  "@media": {
    "(pointer: coarse)": { width: "44PX", height: "44PX" },
    [standardMotion]: {
      transition: `background-color ${settleTransition}`,
    },
    "(forced-colors: active)": { border: "1px solid ButtonText" },
  },
  selectors: {
    "&:hover:not(:disabled)": { background: "#f6e9d329" },
    // Keyboard focus only: a brighter plate with a ring around it.
    "&:focus-visible": {
      outline: "none",
      background: "#f5e9b93d",
      boxShadow: `0 0 0 2px ${t.color.focus}`,
    },
    "&:disabled": { opacity: 0.45, cursor: "default" },
  },
});
globalStyle(`${close} svg`, { width: "28px", height: "28px" });
// With the close button leading, the title takes the rest of the row beside it.
globalStyle(`${head}[data-close-side="start"] > div`, { flex: 1, minWidth: 0 });
export const body = style({
  // Sized by its content and able to shrink. A zero basis collapses the body in
  // WebKit releases that resolve it against the dialog's automatic height.
  flex: "1 1 auto",
  minHeight: 0,
  overflow: "auto",
  padding: "22px 32px",
  fontSize: "25px",
  lineHeight: 1.35,
});
export const foot = style({
  flexShrink: 0,
  display: "flex",
  flexWrap: "wrap",
  justifyContent: "flex-end",
  gap: "18px",
  margin: "0 22px",
  padding: "20px 10px 24px",
  borderTop: "1px solid #a698774c",
});

/* The drawer form, used on a phone. */
export const drawerOverlay = style({
  position: "fixed",
  inset: 0,
  zIndex: 1000,
  background: "#05080cb8",
});
export const drawer = style({
  boxSizing: "border-box",
  position: "fixed",
  insetInline: 0,
  bottom: 0,
  zIndex: 1000,
  display: "flex",
  flexDirection: "column",
  // Always leaves a strip of the screen behind it in view.
  maxHeight: "calc(100dvh - 28PX)",
  border: "1.5px solid #e9dcc0",
  borderBottom: 0,
  borderRadius: "24px 24px 0 0",
  background: "linear-gradient(125deg,#0d141b,#0a1016)",
  color: t.color.ink,
  boxShadow: "0 -18px 60px #000000b3, inset 0 0 0 1px #ffffff09",
  fontFamily: t.font.face,
  WebkitFontSmoothing: "antialiased",
  outline: "none",
  paddingBottom: "env(safe-area-inset-bottom)",
  selectors: {
    // A drawer spans the screen whatever width its dialog form is given.
    "&&&": { width: "100%", maxWidth: "100%" },
  },
});
/** The grip that shows a drawer can be pulled down. */
export const drawerHandle = style({
  flex: "none",
  alignSelf: "center",
  width: "44PX",
  height: "5PX",
  marginTop: "10PX",
  borderRadius: "999px",
  background: "#f6e9d359",
  "@media": { "(forced-colors: active)": { background: "CanvasText" } },
});
globalStyle(`${drawer} > ${head}`, { paddingTop: "14px" });
