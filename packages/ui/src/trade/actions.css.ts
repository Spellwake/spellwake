import { globalStyle, style } from "@vanilla-extract/css";
export const group = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,294fr) minmax(0,225fr)",
  alignItems: "center",
  gap: "min(5.9%,33px)",
  boxSizing: "border-box",
  width: "min(100%,552px)",
  minWidth: 0,
  selectors: {
    "&[data-editing]": {
      gridTemplateColumns: "minmax(0,303fr) minmax(0,158fr)",
      gap: "min(6%,30px)",
      width: "min(100%,491px)",
    },
  },
});
export const primary = style({
  selectors: {
    "&&": {
      minWidth: 0,
      justifyContent: "center",
      height: 57,
      padding: "4px 6px",
      border: "2px solid #e1d3b7",
      borderRadius: 14,
      color: "#f3e8ce",
      background: "linear-gradient(168deg,#302d29 0%,#202021 36%,#111517 90%)",
      boxShadow: "inset 0 1px 2px #fff3,0 2px 5px #0008",
      textShadow: "0 1px 3px #000",
      font: '32px/1.1 "Times New Roman",serif',
    },
    "&&:disabled": {
      opacity: 1,
      border: "1px solid #787771",
      background: "linear-gradient(165deg,#2d2b2b,#202122 75%,#191a1a)",
      color: "#9a9691",
    },
  },
});
export const cancel = style({
  selectors: {
    "&&": {
      minWidth: 0,
      height: 47,
      padding: "4px 5px",
      justifyContent: "center",
      border: "2px solid #b39478",
      borderRadius: 12,
      background: "linear-gradient(160deg,#1a1d20,#0f1419)",
      color: "#f2e4cd",
      font: '25px/1.1 "Times New Roman",serif',
    },
  },
});
globalStyle(`${group}[data-editing] ${primary}`, { fontSize: 29, height: 56 });
globalStyle(`${group}[data-editing] ${cancel}`, {
  paddingInlineStart: 22,
  borderColor: "transparent",
  background: "transparent",
  boxShadow: "none",
  color: "#f1d9c2",
});
// Ready, Trade and Cancel trade never drop below a fingertip on touch screens.
globalStyle(`${primary}${primary}, ${cancel}${cancel}`, {
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
