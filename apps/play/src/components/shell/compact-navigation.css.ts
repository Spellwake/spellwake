import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { glassEdge, glassSurface, TOUCH_TARGET } from "./glass.css";
import { shellTokens } from "./shell.css";

const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "-4px",
} as const;

/* Section strip under the header. */
export const strip = style({
  position: "relative",
  zIndex: 1,
  flexShrink: 0,
  display: "flex",
  ...glassSurface,
  paddingInline:
    "max(10px, env(safe-area-inset-left, 0px)) max(10px, env(safe-area-inset-right, 0px))",
  borderBottom: `1px solid ${glassEdge}`,
});
/** The same sections sharing the header's glass instead of a bar of their own. */
export const stripInline = style({
  flex: 1,
  display: "flex",
  alignSelf: "stretch",
  maxWidth: "520px",
  marginInline: "auto",
});
export const stripLink = style({
  position: "relative",
  flex: 1,
  display: "grid",
  placeItems: "center",
  minHeight: `max(64px, ${TOUCH_TARGET})`,
  color: shellTokens.ink,
  fontSize: "24px",
  textDecoration: "none",
  selectors: {
    "&[aria-current=page]": { color: shellTokens.active },
    "&:focus-visible": focus,
  },
});
/** The desktop header's gold line and diamond, carried onto the strip. */
export const stripMark = style({
  position: "absolute",
  bottom: "-1px",
  insetInline: "18%",
  height: "2px",
  background: `linear-gradient(90deg,transparent,${shellTokens.line} 22%,${shellTokens.line} 78%,transparent)`,
  "@media": { "(forced-colors: active)": { background: "Highlight" } },
  selectors: {
    "&::before": {
      content: '""',
      position: "absolute",
      left: "50%",
      top: "50%",
      width: "7px",
      height: "7px",
      background: "#f2d8a9",
      transform: "translate(-50%,-50%) rotate(45deg)",
      boxShadow: `0 0 10px 1px ${shellTokens.glow}99`,
    },
  },
});

/* Floating destination dock. */
export const dock = style({
  position: "absolute",
  zIndex: 3,
  left: "50%",
  bottom: "calc(22px + env(safe-area-inset-bottom))",
  transform: "translateX(-50%)",
  display: "flex",
  gap: "6px",
  padding: "8px",
  borderRadius: "999px",
  ...glassSurface,
  border: `1px solid ${glassEdge}`,
  boxShadow: "0 18px 44px #00000099, inset 0 1px 0 #f6e9d31f",
  selectors: {
    // On its side a phone has no height to spare, so the dock stands on the left edge.
    ':root[data-orientation="landscape"] &': {
      left: "calc(14px + env(safe-area-inset-left, 0px))",
      bottom: "auto",
      // Centred in the space below the header.
      top: `calc(50% + ${shellTokens.compactBar} / 2)`,
      transform: "translateY(-50%)",
      flexDirection: "column",
    },
  },
});
export const dockItem = style({
  position: "relative",
  isolation: "isolate",
  display: "flex",
  alignItems: "center",
  gap: "10px",
  minWidth: `max(66px, ${TOUCH_TARGET})`,
  minHeight: `max(66px, ${TOUCH_TARGET})`,
  padding: "0 17px",
  borderRadius: "999px",
  color: shellTokens.ink,
  fontSize: "21px",
  textDecoration: "none",
  whiteSpace: "nowrap",
  selectors: {
    "&[aria-current=page]": {
      color: shellTokens.active,
      paddingInline: "22px",
    },
    ':root[data-orientation="landscape"] &': {
      justifyContent: "center",
      paddingInline: "17px",
      minWidth: `max(58px, ${TOUCH_TARGET})`,
      minHeight: `max(58px, ${TOUCH_TARGET})`,
    },
    "&:focus-visible": { ...focus, outlineOffset: "2px" },
  },
});
export const dockFill = style({
  position: "absolute",
  inset: 0,
  zIndex: -1,
  borderRadius: "999px",
  background: "linear-gradient(104deg,#5a4526b3,#3a3324a6 50%,#22302f99)",
  border: `1px solid ${shellTokens.line}80`,
  boxShadow: `0 0 18px ${shellTokens.glow}2e, inset 0 1px 0 #fff1d61f`,
  "@media": {
    "(forced-colors: active)": { border: "2px solid Highlight" },
  },
});
export const dockLabel = style({
  selectors: { ':root[data-orientation="landscape"] &': { display: "none" } },
});
export const glyph = style({ width: "32px", height: "32px", display: "grid" });
globalStyle(`${glyph} > svg`, {
  width: "100%",
  height: "100%",
  strokeWidth: 1.35,
});
