import { style, globalStyle } from "@vanilla-extract/css";
const text = {
  color: "#f4e6c7",
  fontFamily: '"Iowan Old Style", Georgia, serif',
  textShadow: "0 1px 2px #050a0d",
  WebkitFontSmoothing: "antialiased",
} as const;
export const identity = style({
  ...text,
  display: "inline-flex",
  alignItems: "center",
  gap: 14,
  minWidth: 0,
  lineHeight: 1.06,
});
export const emblem = style({
  display: "inline-grid",
  gridTemplate: "minmax(0, 1fr) / minmax(0, 1fr)",
  placeItems: "center",
  flex: "none",
  width: 98,
  height: 92,
});
globalStyle(`${emblem} > *`, {
  width: "100%",
  height: "100%",
  objectFit: "contain",
});
export const copy = style({ display: "grid", gap: 4 });
export const tier = style({ fontSize: 33, fontWeight: 600 });
export const rating = style({ fontSize: 22, letterSpacing: "-.3px" });
globalStyle(`${identity}[data-presentation="standings"] ${emblem}`, {
  width: 88,
  height: 83,
});
globalStyle(`${identity}[data-presentation="profile"] ${emblem}`, {
  width: 78,
  height: 70,
});
globalStyle(`${identity}:not([data-presentation="season"]) ${tier}`, {
  transform: "translate(2px,-2px)",
  fontSize: 32,
  fontWeight: 400,
});
globalStyle(`${identity}[data-presentation="profile"] ${tier}`, {
  transform: "translate(3px,-4px)",
  fontSize: 29,
});
export const progress = style({ display: "grid", gap: 6, width: "100%" });
export const track = style({
  position: "relative",
  display: "block",
  height: 18,
  boxSizing: "border-box",
  borderRadius: 12,
  border: "1px solid #b5a085",
  background: "linear-gradient(#070f15,#17242a)",
  boxShadow: "inset 0 1px 3px #03070b",
});
export const glow = style({
  position: "absolute",
  inset: 0,
  filter: "drop-shadow(0 0 5px #ffbd4c99)",
});
export const mask = style({
  position: "absolute",
  inset: 0,
  overflow: "hidden",
  borderRadius: 9,
});
export const fill = style({
  position: "absolute",
  inset: 0,

  borderRadius: "inherit",
  background: "linear-gradient(#ffe4a0,#f4be56 43%,#f8c75d 76%,#ffdc85)",
  boxShadow: "inset 0 0 0 1px #ffdf9988",
});
export const progressLabel = style({ ...text, fontSize: 24, lineHeight: 1.1 });
