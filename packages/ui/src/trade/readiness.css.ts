import { style } from "@vanilla-extract/css";
import { controlTokens } from "../controls.css";

export const badge = style({
  boxSizing: "border-box",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  verticalAlign: "middle",
  borderRadius: 999,
  border: "1px solid #a7a576",
  paddingInline: 14,
  height: 44,
  color: "#f4efd7",
  textShadow: "0 1px 2px #000, 0 0 0.4px currentColor",
  background:
    "linear-gradient(172deg,#303826 0%,#272e1c 42%,#152015 78%,#1b2819)",
  boxShadow: "inset 0 1px 2px #e5dca947, inset 0 -1px 2px #0c0e09",
  minWidth: 135,
  font: `23px/1 ${controlTokens.font.face}`,
  WebkitFontSmoothing: "antialiased",
  selectors: {
    '&[data-status="editing"]': {
      minWidth: 153,
      height: 42,
      fontSize: 21,
      borderColor: "#968d7e",
      color: "#f0ede2",
      background:
        "linear-gradient(165deg,#272726,#1b1d1d 48%,#101517 83%,#1a1c1c)",
    },
    '&[data-status="confirmed"]': { minWidth: 170, borderColor: "#b5bd74" },
  },
});
export const badgeContent = style({
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 11,
});
export const mark = style({
  width: 26,
  height: 26,
  flex: "none",
  color: "#a3ce79",
  selectors: {
    [`${badge}[data-status="editing"] &`]: {
      width: 20,
      height: 20,
      color: "currentColor",
    },
  },
});
export const statusLine = style({ width: "100%", minWidth: 0 });
export const sentence = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "clamp(8px, 2.5%, 22px)",
  font: 'italic 26px/1.2 "Times New Roman", serif',
  color: "#eee5cf",
  textAlign: "center",
  textShadow: "0 1px 3px #000",
  WebkitFontSmoothing: "antialiased",
  selectors: {
    "&::before, &::after": {
      content: '""',
      flex: "1 1 0",
      height: 2,
      background:
        "linear-gradient(90deg,#c8b599 0%,#e6d4af 75%,#c8b599 93%,transparent)",
    },
    "&::after": { transform: "scaleX(-1)" },
  },
});
export const words = style({
  minWidth: 0,
  maxWidth: "90%",
  position: "relative",
  top: -2,
  overflowWrap: "anywhere",
});
