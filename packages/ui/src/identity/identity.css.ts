import { globalStyle, style } from "@vanilla-extract/css";
import { feedbackTransition, standardMotion } from "../control-motion";
export const avatar = style({
  position: "relative",
  display: "inline-flex",
  width: "var(--sw-avatar-size)",
  height: "var(--sw-avatar-size)",
  flex: "none",
  borderRadius: "50%",
  boxSizing: "border-box",
  padding: "max(1px, calc(var(--sw-avatar-size) * .011))",
  background: "linear-gradient(135deg,#aaa492,#4c4b45 45%,#929283)",
  verticalAlign: "middle",
  selectors: {
    '&[data-ring="gold"]': {
      background:
        "linear-gradient(135deg,#fff3bb,#ead58c 32%,#b59b57 62%,#fff0af)",
      boxShadow: "0 0 0 1px #171611,0 0 4px #0b0909",
    },
    '&[data-ring="selected"]': {
      background:
        "linear-gradient(138deg,#fff5c8,#ffda82 52%,#b48845 84%,#fff2cf)",
    },
  },
});
export const halo = style({
  position: "absolute",
  inset: 0,
  borderRadius: "inherit",
  pointerEvents: "none",
  boxShadow: "0 0 2px 1px #ffe6ad, 0 0 7px #eeb64678",
});
export const art = style({
  display: "block",
  width: "100%",
  height: "100%",
  borderRadius: "inherit",
  overflow: "hidden",
  background: "#252822",
});
globalStyle(`${art} > img, ${art} > svg`, {
  display: "block",
  width: "100%",
  height: "100%",
  objectFit: "cover",
});
export const identity = style({
  position: "relative",
  display: "flex",
  alignItems: "center",
  boxSizing: "border-box",
  minWidth: 0,
  margin: 0,
  padding: 0,
  border: 0,
  background: "transparent",
  color: "#f5ebd6",
  font: '22px/1.15 "Iowan Old Style", Georgia, serif',
  WebkitFontSmoothing: "antialiased",
  textAlign: "start",
  textShadow: "0 1px 2px #090a09",
  selectors: {
    "button&": { cursor: "pointer" },
    "&:focus-visible": { outline: "2px solid #ffe6ae", outlineOffset: -2 },
  },
  "@media": {
    [standardMotion]: {
      transition: `box-shadow ${feedbackTransition}, border-color ${feedbackTransition}, background-color ${feedbackTransition}`,
    },
  },
});
export const copy = style({ minWidth: 0, display: "grid", gap: 2 });
export const name = style({
  display: "block",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const detail = style({
  display: "block",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "#e6cca2",
});
export const layouts = {
  standings: style({
    fontSize: 35,
    gap: 27,
    fontWeight: 400,
    borderRadius: 4,
    selectors: { "&:hover": { backgroundColor: "#eee4c70d" } },
  }),
  profile: style({ fontSize: 64, gap: 37, fontWeight: 400 }),
  heading: style({ fontSize: 28 }),
  tray: style({
    font: '36px/1.15 Baskerville, "Times New Roman", serif',
    gap: 22,
    color: "#f1ece3",
  }),
  request: style({
    width: "100%",
    height: 77,
    gap: 24,
    padding: "4px 16px",
    fontSize: 24,
    border: "1px solid #43403b",
    borderRadius: 13,
    background: "linear-gradient(110deg,#262525,#232324 65%,#282726)",
    selectors: {
      "&:hover": {
        borderColor: "#8a785d",
        boxShadow: "inset 0 0 18px #b6aa8120",
      },
    },
  }),
  seat: style({
    height: 51,
    minWidth: 162,
    justifyContent: "flex-start",
    gap: 16,
    padding: "3px 9px",
    borderRadius: 30,
    border: "1px solid #68695f",
    font: '500 18px/1.15 "Albert Sans",system-ui,sans-serif',
    color: "#fff3d8",
    background: "linear-gradient(115deg,#30362e,#242923)",
    selectors: {
      "&[data-turn]": {
        minWidth: 166,
        borderColor: "#ffdf91",
        boxShadow: "0 0 13px #eed17a57,inset 0 0 9px #edd9a72b",
        background: "linear-gradient(115deg,#504936,#2c2c22)",
      },
      "&[data-viewed]": {
        borderColor: "#fff8db",
        boxShadow: "inset 0 0 0 1px #fff4d8",
      },
      "&:hover": { backgroundColor: "#444a3c", filter: "brightness(1.08)" },
    },
  }),
};
globalStyle(`${layouts.request} ${copy}`, { flex: 1 });
globalStyle(`${layouts.request} ${detail}`, {
  fontSize: 20,
  fontStyle: "italic",
});
globalStyle(`${layouts.heading} ${detail}`, { fontSize: 20 });

export const seatArt = style({
  display: "inline-flex",
  position: "relative",
  alignItems: "center",
});
export const eye = style({
  position: "absolute",
  left: "calc(100% - 3px)",
  bottom: -1,
  display: "grid",
  placeItems: "center",
  borderRadius: "50%",
  background: "#283025",
  color: "#fff2d5",
});
export const caption = style({
  position: "absolute",
  top: "100%",
  left: 0,
  right: 0,
  marginTop: 3,
  textAlign: "center",
  font: 'italic 600 15px/1.3 "Albert Sans",system-ui,sans-serif',
  color: "#fff6dc",
});
globalStyle(`${layouts.standings} ${copy}, ${layouts.profile} ${copy}`, {
  transform: "translateY(-3px)",
});
globalStyle(`${avatar}[data-ring="gold"] ${art}`, {
  outline: "1px solid #171611",
  outlineOffset: -1,
});
