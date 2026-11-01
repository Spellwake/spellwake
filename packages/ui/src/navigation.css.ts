import { style } from "@vanilla-extract/css";
import { vars } from "./theme.css";

export const topbar = style({
  display: "flex",
  alignItems: "stretch",
  minHeight: 80,
  width: "100%",
  background: "linear-gradient(90deg, rgba(5,10,17,.96), rgba(10,15,24,.76))",
  backdropFilter: "blur(20px)",
  borderBottom: `1px solid ${vars.color.line}`,
  position: "relative",
  zIndex: 10,
  "@media": { "(max-width: 900px)": { flexWrap: "wrap" } },
});
export const brand = style({
  width: 132,
  minWidth: 90,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  padding: "12px 24px",
  borderRight: "1px solid rgba(210,195,166,.10)",
  "@media": { "(max-width: 900px)": { width: 90, minWidth: 72 } },
});
export const logo = style({ width: 45, height: 54, objectFit: "contain" });
export const links = style({
  display: "flex",
  alignItems: "stretch",
  gap: 2,
  flex: 1,
  minWidth: 0,
  "@media": {
    "(max-width: 900px)": {
      order: 3,
      width: "100%",
      flexBasis: "100%",
      overflowX: "auto",
      minHeight: 54,
    },
  },
});
export const topItem = style({
  border: 0,
  background: "transparent",
  color: vars.color.muted,
  fontFamily: vars.font.display,
  fontSize: 23,
  padding: "18px clamp(16px, 2vw, 36px)",
  position: "relative",
  cursor: "pointer",
  whiteSpace: "nowrap",
  selectors: {
    "&:hover": { color: vars.color.text, background: "rgba(210,195,166,.04)" },
    "&[aria-current=page]": {
      color: "#f0d8ad",
      background:
        "linear-gradient(0deg, rgba(192,154,87,.06), transparent 65%)",
    },
    "&[aria-current=page]::after": {
      content: '""',
      position: "absolute",
      height: 2,
      bottom: 0,
      left: 15,
      right: 15,
      background: "#ddbe83",
    },
    "&[aria-current=page]::before": {
      content: '""',
      position: "absolute",
      width: 6,
      height: 6,
      bottom: -2,
      left: "calc(50% - 3px)",
      transform: "rotate(45deg)",
      background: "#edd2a1",
      zIndex: 1,
    },
    "&:focus-visible": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: -5,
    },
  },
  "@media": {
    "(max-width: 900px)": { fontSize: 18, flex: 1, padding: "12px 18px" },
    "(max-width: 640px)": { minWidth: 0, fontSize: 16, padding: "12px 8px" },
  },
});
export const account = style({
  display: "flex",
  alignItems: "center",
  gap: 12,
  marginLeft: "auto",
  padding: "10px 24px",
  "@media": { "(max-width: 640px)": { gap: 2, padding: "8px 10px" } },
});
export const coin = style({
  display: "inline-flex",
  alignItems: "center",
  gap: 9,
  paddingRight: 16,
  borderRight: "1px solid rgba(210,195,166,.17)",
  fontFamily: vars.font.display,
  fontSize: 21,
  fontVariantNumeric: "tabular-nums",
  "@media": { "(max-width: 640px)": { paddingRight: 8, fontSize: 18 } },
});
export const coinIcon = style({ color: "#d8b379" });
export const avatar = style({
  width: 38,
  height: 38,
  borderRadius: "50%",
  objectFit: "cover",
  border: "1px solid #bba273",
  background: "#151b27",
});
export const side = style({
  width: 192,
  flexShrink: 0,
  background: "linear-gradient(90deg,rgba(5,10,17,.91),rgba(9,15,24,.71))",
  backdropFilter: "blur(16px)",
  borderRight: `1px solid ${vars.color.line}`,
  paddingTop: 28,
  minHeight: "100%",
  "@media": {
    "(max-width: 900px)": {
      display: "flex",
      width: "100%",
      minHeight: "auto",
      borderRight: 0,
      borderBottom: `1px solid ${vars.color.line}`,
      padding: "0 8px",
      overflowX: "auto",
    },
  },
});
export const sideItem = style({
  position: "relative",
  display: "flex",
  alignItems: "center",
  gap: 22,
  width: "100%",
  minHeight: 76,
  padding: "18px 28px 18px 36px",
  border: 0,
  borderRadius: 0,
  background: "transparent",
  color: vars.color.muted,
  fontFamily: vars.font.display,
  fontSize: 21,
  textAlign: "left",
  cursor: "pointer",
  selectors: {
    "&:hover": { color: vars.color.text, background: "rgba(206,192,166,.05)" },
    "&[aria-current=page]": {
      color: "#f0d5a5",
      background:
        "linear-gradient(90deg,rgba(171,139,84,.17),rgba(136,114,79,.05))",
    },
    "&[aria-current=page]::after": {
      content: '""',
      position: "absolute",
      top: 0,
      bottom: 0,
      left: 11,
      width: 1,
      background: "#ddbe83",
    },
    "&[aria-current=page]::before": {
      content: '""',
      position: "absolute",
      top: "calc(50% - 3px)",
      left: 9,
      width: 5,
      height: 5,
      background: "#edcf94",
      transform: "rotate(45deg)",
    },
    "&:focus-visible": {
      outline: `2px solid ${vars.color.goldMuted}`,
      outlineOffset: -5,
    },
  },
  "@media": {
    "(max-width: 900px)": {
      minHeight: 55,
      justifyContent: "center",
      gap: 10,
      padding: "12px 22px",
      fontSize: 17,
      whiteSpace: "nowrap",
    },
  },
});
export const sideIcon = style({
  display: "inline-flex",
  width: 24,
  flexShrink: 0,
});
