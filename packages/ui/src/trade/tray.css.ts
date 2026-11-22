import { style, globalStyle } from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";

import * as item from "./itemRow.css";

export const tray = style({
  height: "100%",
  width: "100%",
  boxSizing: "border-box",
  minHeight: 0,
  minWidth: 0,
  display: "grid",
  gridTemplateRows: "auto minmax(0,1fr)",
  padding: "16px 23px 19px 33px",
  border: "1px solid #c5baa5",
  borderRadius: 21,
  containerType: "size",
  containerName: "trade-tray",
  background: "linear-gradient(117deg,#0b1014,#0b121a 55%,#090f16)",
  boxShadow: "inset 0 0 0 1px #5a585366",
});
export const header = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 10,
  minWidth: 0,
  paddingBottom: 11,
  position: "relative",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      bottom: 0,
      left: 0,
      right: 0,
      height: 2,
      background:
        "linear-gradient(90deg,#a99d87,#8e7a64 24%,#c5b59b 66%,#817666)",
    },
  },
  "@container": { "(max-width: 400px)": { flexWrap: "wrap" } },
});
export const scroll = style({
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
  overflowY: "auto",
  overscrollBehaviorY: "contain",
  scrollbarColor: "#978a75 #11171b",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: -2,
    },
  },
});
export const items = style({
  flexShrink: 0,
  listStyle: "none",
  margin: 0,
  padding: 0,
});
export const empty = style({
  display: "grid",
  placeItems: "center",
  selectors: {
    "&[data-space]": { flex: "1 0 auto", minHeight: 54 },
  },
});

// Align the identity columns when card and pack rows share one tray.
globalStyle(`${items} ${item.row}:is([data-kind="card"], [data-editable])`, {
  "@container": {
    "(min-width: 441px)": { gridTemplateColumns: "25% minmax(0,1fr) auto" },
  },
});
globalStyle(`${items} ${item.row}[data-kind="pack"]:not([data-editable])`, {
  "@container": { "(min-width: 441px)": { paddingTop: 18 } },
});
globalStyle(
  `${items} ${item.row}[data-kind="pack"]:not([data-editable]) :is(${item.identity}, ${item.trailing})`,
  {
    "@container": { "(min-width: 441px)": { position: "relative", top: 4 } },
  },
);

globalStyle(`${tray}:is([data-editable],[data-locked])`, {
  gridTemplateRows: "auto minmax(0,1fr) auto",
});
export const footer = style({
  display: "grid",
  justifyItems: "center",
  alignContent: "start",
  paddingTop: 22,
  paddingBottom: 6,
});
export const add = style({
  selectors: {
    "&&": {
      width: "min(259px, 100%)",
      height: 51,
      justifyContent: "center",
      fontSize: 25,
      borderRadius: 9,
      borderColor: "#b5a58c",
      background: "linear-gradient(#292928,#121416)",
      color: t.color.ink,
    },
  },
});
globalStyle(`${tray}[data-compact] ${footer}`, {
  paddingTop: 5,
  paddingBottom: 0,
});
globalStyle(`${tray}[data-compact] ${add}`, { height: 40 });
globalStyle(`${tray}[data-compact] ${scroll}`, {
  scrollbarGutter: "stable",
});
globalStyle(`${tray}[data-compact] ${items} ${item.row}`, {
  "@container": {
    "(min-width: 441px)": {
      gridTemplateColumns: "20% minmax(0,1fr) auto",
      minHeight: 74,
      padding: "4px 3px",
      columnGap: "2%",
    },
  },
});
globalStyle(`${tray}[data-compact] ${item.identity}`, {
  minHeight: "2.32em",
  gap: 0,
  paddingLeft: 16,
});
globalStyle(`${tray}[data-compact] ${item.editor}`, { transform: "none" });
globalStyle(
  `${tray}[data-compact] ${items} ${item.row}[data-kind="coin"] img`,
  { width: 59 },
);
globalStyle(`${tray}[data-compact]`, { paddingBottom: 13 });
globalStyle(`${tray}[data-compact] ${scroll}`, {
  paddingTop: 5,
  paddingRight: 16,
  scrollbarWidth: "auto",
  scrollbarColor: "#d2b891 #282726",
  // Compact trays show a persistent native track even on overlay-scrollbar platforms.
  "@supports": { "selector(::-webkit-scrollbar)": { scrollbarColor: "auto" } },
});
globalStyle(`${tray}[data-compact] ${scroll}::-webkit-scrollbar`, {
  width: 12,
});
globalStyle(`${tray}[data-compact] ${scroll}::-webkit-scrollbar-track`, {
  background: "#282726",
  borderRadius: 8,
});
globalStyle(`${tray}[data-compact] ${scroll}::-webkit-scrollbar-thumb`, {
  background: "#d2b891",
  borderRadius: 8,
});

// Keep the coin editor and primary action reachable in a short offer panel.
const shortTray = "trade-tray (min-width: 500px) and (max-height: 415px)";
globalStyle(`${tray} ${footer}`, {
  "@container": { [shortTray]: { paddingTop: 16, paddingBottom: 0 } },
});
globalStyle(`${tray} ${add}`, {
  "@container": { [shortTray]: { width: "min(328px,100%)" } },
});
globalStyle(`${tray} ${item.row}[data-editable][data-kind=coin]`, {
  "@container": { [shortTray]: { paddingBlock: 4 } },
});

export const ready = style({
  selectors: {
    "&&": {
      justifyContent: "center",
      width: "min(350px,100%)",
      height: 56,
      borderRadius: 13,
      font: '32px/1 "Times New Roman",serif',
    },
    "&&:disabled": {
      opacity: 1,
      color: "#bbaa94",
      background: "linear-gradient(135deg,#302e2b,#242524)",
      borderColor: "#c3ae8c",
      boxShadow: "inset 0 0 6px #92857427",
    },
  },
});

globalStyle(`${tray}[data-ready-action]`, { paddingTop: 6 });
globalStyle(`${tray}[data-ready-action] ${footer}`, {
  gridTemplateColumns: "repeat(2,minmax(0,1fr))",
  alignItems: "center",
  gap: 12,
  paddingBottom: 0,
});

export const edit = style([
  add,
  { selectors: { "&&": { width: "min(243px,100%)", height: 52 } } },
]);
globalStyle(`${tray}[data-locked] ${footer}`, {
  paddingBottom: 8,
  paddingTop: 16,
});
// Add items and Edit items never drop below a fingertip on touch screens.
globalStyle(`${add}${add}, ${edit}${edit}`, {
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
