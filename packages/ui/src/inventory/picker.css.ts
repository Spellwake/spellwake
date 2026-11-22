import { globalStyle, style } from "@vanilla-extract/css";
export const srOnly = style({
  position: "absolute",
  width: 1,
  height: 1,
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
  margin: -1,
  padding: 0,
  border: 0,
});
export const picker = style({
  containerType: "inline-size",
  display: "block",
  boxSizing: "border-box",
  padding: 0,
  margin: 0,
  border: 0,
  minWidth: 0,
  width: "100%",
  color: "#f1eadf",
  font: '400 20px/1.18 "Times New Roman",serif',
  WebkitFontSmoothing: "antialiased",
  textShadow: "0 1px 2px #030505",
  selectors: {
    "&[data-mode=add]": {
      border: "1px solid #46494a",
      borderRadius: 11,
      background: "linear-gradient(105deg,#0d1315,#0b1012)",
    },
  },
});
export const row = style({
  display: "flex",
  alignItems: "center",
  boxSizing: "border-box",
  position: "relative",
  isolation: "isolate",
  minWidth: 0,
  minHeight: 77,
  padding: "6px 13px 6px 17px",
  gap: 0,
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      height: 1,
      inset: "auto 1% 0",
      background:
        "linear-gradient(90deg,transparent,#494d4b 5%,#3d4343 95%,transparent)",
    },
    [`${picker}[data-mode=select] &`]: { cursor: "pointer" },
    [`${picker}[data-mode=select] &[data-kind=pack]`]: {
      minHeight: 67,
      padding: "4px 10px 4px 19px",
    },
    [`${picker}[data-mode=add] &`]: { minHeight: 73, padding: "5px 15px" },
    [`${picker}[data-mode=add] &[data-kind=pack]`]: {
      minHeight: 96,
      padding: "4px 15px",
    },
    [`${picker}[data-mode=add] &:last-child::after`]: { content: "none" },
    [`${picker}[data-mode=add] &::after`]: { left: "23%", right: 14 },
    [`${picker}[data-mode=add] &[data-kind=pack]::after`]: { left: 14 },
    "&[data-disabled=true]": { cursor: "default", color: "#c5bdaf" },
    "&:has(input:focus-visible)": {
      outline: "2px solid #ffe5a8",
      outlineOffset: -3,
      borderRadius: 10,
    },
  },
});
export const radio = style([srOnly]);
export const highlight = style({
  position: "absolute",
  zIndex: -1,
  inset: 0,
  border: "2px solid #e7dbb9",
  borderRadius: 11,
  background: "linear-gradient(115deg,#191d1d,#141919)",
  boxShadow: "inset 0 0 0 2px #101415,0 0 4px #c1ae6d30",
  pointerEvents: "none",
});
export const art = style({
  width: 80,
  height: 62,
  flex: "0 1 80px",
  minWidth: 32,
  display: "grid",
  placeItems: "center",
  overflow: "hidden",
  selectors: {
    [`${row}[data-kind=pack] &`]: { height: 60, width: 56, flexBasis: 56 },
    [`${picker}[data-mode=add] &`]: { height: 60, width: 103, flexBasis: 103 },
    [`${picker}[data-mode=add] ${row}[data-kind=pack] &`]: {
      height: 86,
      width: 96,
      flexBasis: 96,
    },
  },
});
globalStyle(`${art} > :is(img,svg)`, {
  maxWidth: "100%",
  maxHeight: "100%",
  display: "block",
  objectFit: "contain",
});
export const identity = style({
  display: "flex",
  flexDirection: "column",
  justifyContent: "center",
  flex: "1 1 0",
  minWidth: 0,
  overflowWrap: "anywhere",
  gap: 4,
  marginInline: "clamp(7px,4%,25px) 8px",
  selectors: {
    [`${picker}[data-mode=add] &`]: { marginInlineStart: 21 },
    [`${picker}[data-mode=select] ${row}[data-kind=pack] &`]: {
      marginInlineStart: "clamp(9px,6%,32px)",
    },
  },
});
export const name = style({
  fontSize: 23,
  lineHeight: 1.13,
  selectors: { [`${row}[data-kind=pack] &`]: { fontSize: 21 } },
});
export const detail = style({
  fontSize: 19,
  color: "#e4dccc",
  selectors: { [`${picker}[data-mode=add] &`]: { fontSize: 17 } },
});
export const availability = style({
  display: "inline-flex",
  alignItems: "center",
  flex: "none",
  gap: 14,
  whiteSpace: "nowrap",
});
export const stock = style({
  fontSize: 20,
  selectors: {
    [`${picker}[data-mode=add] &`]: {
      borderInlineStart: "1px solid #555957",
      padding: "7px 0 7px 17px",
      fontSize: 18,
    },
  },
});
export const added = style({
  fontSize: 15,
  padding: "3px 15px",
  borderRadius: 20,
  border: "1px solid #736858",
  background: "linear-gradient(#40392d,#322b22)",
});
export const check = style({
  width: 28,
  display: "grid",
  placeItems: "center",
  marginInlineStart: 10,
  color: "#fff6df",
});
export const addButton = style({
  selectors: {
    "&&": {
      flex: "none",
      padding: 0,
      width: 39,
      minWidth: 0,
      height: 35,
      marginInlineStart: 20,
      borderRadius: 9,
      border: "1px solid #bb9f79",
      background: "linear-gradient(125deg,#1c2020,#101314)",
      boxShadow: "inset 0 0 0 1px #534a3a66",
      color: "#f6f0e4",
    },
    "&&:disabled": { borderColor: "#514d46", opacity: 0.5 },
  },
});

globalStyle(`${picker}[data-mode=add] ${row}[data-kind=pack] ${identity}`, {
  marginInlineStart: 12,
  position: "relative",
  top: -12,
});
globalStyle(`${picker}[data-mode=select] ${row}[data-kind=card] ${identity}`, {
  marginInlineStart: "clamp(8px,5%,28px)",
});
globalStyle(`${row}[data-kind=pack] ${check}`, {
  width: 23,
  marginInlineStart: 4,
});
globalStyle(`${addButton} svg`, { width: 25, height: 25 });

// Give stock its own line when the picker is embedded in a narrow panel.
const narrow = "(max-width: 420px)";
globalStyle(`${picker} ${row}`, {
  "@container": {
    [narrow]: {
      display: "grid",
      gridTemplateColumns: "min(18%,60px) minmax(0,1fr) auto",
      gap: "4px 9px",
    },
  },
});
globalStyle(`${row} ${art}`, {
  "@container": {
    [narrow]: { gridColumn: "1", gridRow: "1 / span 2", maxWidth: "100%" },
  },
});
globalStyle(`${picker}[data-mode] ${row}[data-kind] ${identity}`, {
  "@container": {
    [narrow]: { gridColumn: "2", gridRow: "1", margin: 0, top: 0 },
  },
});
globalStyle(`${row} ${availability}`, {
  "@container": {
    [narrow]: { gridColumn: "2", gridRow: "2", flexWrap: "wrap", gap: 5 },
  },
});
globalStyle(`${picker}[data-mode] ${stock}`, {
  "@container": { [narrow]: { border: 0, padding: 0, fontSize: 16 } },
});
globalStyle(`${row} :is(${check},${addButton})`, {
  "@container": {
    [narrow]: { gridColumn: "3", gridRow: "1 / span 2", marginInlineStart: 0 },
  },
});
// Add keeps its square on touch screens, at a fingertip's size.
globalStyle(`${addButton}${addButton}`, {
  "@media": { "(pointer: coarse)": { width: "44PX", height: "44PX" } },
});
