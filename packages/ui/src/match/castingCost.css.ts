import { globalStyle, style } from "@vanilla-extract/css";
const face = 'Baskerville,"Times New Roman",serif';
export const cost = style({
  fontFamily: face,
  WebkitFontSmoothing: "antialiased",
  fontSize: 24,
  lineHeight: 1.1,
  color: "#fcf3df",
  textShadow: "0 2px 3px #121a0b",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 19,
  selectors: {
    '&[data-size="payment"]': {
      fontSize: 36,
      gap: 12,
      letterSpacing: ".025em",
    },
  },
});
export const symbols = style({
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
  flexWrap: "wrap",
});
globalStyle(`${symbols} img`, {
  display: "block",
  width: 30,
  height: 30,
  filter: "drop-shadow(0 2px 2px #091005a6)",
});
globalStyle(`${cost}[data-size="payment"] img`, { width: 42, height: 42 });
export const picker = style({
  width: 282,
  maxWidth: "100%",
  color: "#fff4dc",
  fontFamily: face,
});
globalStyle(`${picker} ${cost}`, { paddingInlineEnd: 20 });
export const row = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
  padding: "0 0 7px 15px",
  marginBottom: 8,
  borderBottom: "1px solid #b3ad93",
});
export const x = style({
  fontSize: 22,
  textShadow: "0 1px 3px #000",
  lineHeight: 1,
});
export const max = style({
  selectors: {
    "&&": {
      marginRight: -5,
      height: 35,
      minHeight: 0,
      minWidth: 70,
      padding: "0 10px",
      borderRadius: 25,
      font: `400 21px/1 ${face}`,
      border: "1.5px solid #d7bf89",
      boxShadow: "inset 0 1px 2px #eee2ab33,0 1px 3px #0004",
      background: "linear-gradient(#30442d,#223522)",
      color: "#fff0d0",
      textShadow: "0 1px 3px #000",
    },
  },
});
