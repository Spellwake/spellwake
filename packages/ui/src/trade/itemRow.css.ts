import { style, globalStyle } from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";

export const container = style({ containerType: "inline-size", width: "100%" });
export const row = style({
  boxSizing: "border-box",
  display: "grid",
  alignItems: "center",
  gridTemplateColumns: "24% minmax(0,1fr) auto",
  columnGap: "3%",
  padding: "17px 3px 14px 0",
  minHeight: 130,
  color: t.color.ink,
  borderBottom: "1px solid #454944",
  font: `25px/1.14 ${t.font.face}`,
  WebkitFontSmoothing: "antialiased",
  textShadow: "0 0 .45px currentColor, 0 1px 2px #000",
  selectors: {
    '&[data-kind="pack"]:not([data-editable])': {
      gridTemplateColumns: "25% minmax(0,1fr) auto",
      paddingBlock: "12px",
      minHeight: 200,
      alignItems: "start",
      borderBottom: 0,
    },
    '&[data-kind="coin"][data-editable]': { minHeight: "auto" },
    '&[data-kind="coin"]:not([data-editable])': {
      gridTemplateColumns: "20% minmax(0,1fr) auto",
      minHeight: 108,
      paddingBlock: 12,
    },
  },
  "@container": {
    "(max-width: 440px)": {
      gridTemplateColumns: "65px minmax(0,1fr)",
      gap: 10,
      padding: "10px 0",
    },
  },
});
export const art = style({
  display: "grid",
  placeItems: "center",
  minWidth: 0,
});
globalStyle(`${art} > :is(img,svg)`, {
  display: "block",
  maxWidth: "100%",
  height: "auto",
});
export const identity = style({
  display: "grid",
  alignContent: "center",
  gap: 7,
  minHeight: 72,
  borderLeft: "1px solid #8c8472",
  paddingLeft: "clamp(8px,3cqi,22px)",
  overflowWrap: "anywhere",
  selectors: {
    [`${row}[data-kind="pack"]:not([data-editable]) &`]: {
      marginTop: 14,
      alignContent: "start",
      paddingTop: 4,
    },
    [`${row}[data-kind="coin"]:not([data-editable]) &`]: { minHeight: 54 },
  },
});
export const detail = style({ fontSize: "0.84em", color: "#e4dbca" });
export const trailing = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "end",
  selectors: {
    [`${row}[data-kind="pack"]:not([data-editable]) &`]: { marginTop: 34 },
  },
  "@container": { "(max-width: 440px)": { gridColumn: "1/-1" } },
});
export const count = style({
  whiteSpace: "nowrap",
  paddingRight: 7,
  transform: "translateY(-3px)",
  fontVariantNumeric: "lining-nums tabular-nums",
  fontSize: 29,
});
export const editor = style({
  display: "grid",
  transform: "translateY(-4px)",

  gridTemplateColumns: "auto auto",
  alignItems: "center",
  columnGap: "clamp(10px,4.4cqi,29px)",
});
export const remove = style({
  selectors: {
    "&&": {
      width: 42,
      height: 40,
      minWidth: 0,
      padding: 0,
      justifyContent: "center",
      borderRadius: 9,
      borderColor: "#968b79",
      background: "linear-gradient(#242526,#101516 65%,#191b19)",
    },
  },
});
// Remove keeps the amount's height on touch screens, where both grow to a fingertip.
globalStyle(`${remove}${remove}`, {
  "@media": {
    "(pointer: coarse)": {
      width: "max(42px, 44PX)",
      height: "max(40px, 44PX)",
    },
  },
});
