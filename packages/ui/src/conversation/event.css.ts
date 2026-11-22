import { globalStyle, style } from "@vanilla-extract/css";

export const card = style({
  boxSizing: "border-box",
  width: "100%",
  containerType: "inline-size",
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr)",
  alignItems: "center",
  padding: "8px 10px 8px 16px",
  columnGap: 14,
  rowGap: 6,
  border: "1px solid #3c3b39",
  borderRadius: 13,
  background: "linear-gradient(120deg, #242422,#29292b 52%,#232322)",
  boxShadow: "inset 0 1px 1px #ffffff06",
  font: '400 21px/1.2 Georgia,"Times New Roman",serif',
  color: "#f1ece3",
  textShadow: "0 1px 2px #101010",
  WebkitFontSmoothing: "antialiased",
  selectors: {
    '&[data-kind="friend"], &[data-resolved=true]': {
      padding: "12px 12px 11px 26px",
      columnGap: 20,
      rowGap: 7,
    },
    '&[data-kind="game"][data-resolved=false]': { paddingBlock: 10 },
    "&[data-resolved=true]": { minHeight: 66 },
    '&[data-kind="game"][data-resolved=true]': {
      minHeight: 90,
      rowGap: 3,
      paddingBlock: 9,
    },
    "&[data-blocked=true]": { color: "#b4b1ac" },
  },
});
export const symbol = style({
  width: 30,
  height: 26,
  color: "#e3c16f",
  selectors: {
    [`${card}[data-kind="game"] &`]: { width: 42 },
    [`${card}[data-kind="friend"] &`]: { width: 40, height: 35 },
    [`${card}[data-kind="game"][data-resolved=true] &`]: {
      gridRow: "1 / span 2",
      height: 34,
    },
  },
});
export const heading = style({
  display: "flex",
  flexWrap: "wrap",
  alignItems: "baseline",
  justifyContent: "space-between",
  gap: "2px 8px",
  paddingInlineEnd: 4,
  selectors: {
    [`${card}[data-kind="game"][data-resolved=true] &`]: {
      flexDirection: "column-reverse",
      alignItems: "stretch",
    },
  },
});
globalStyle(`${heading} h3`, { all: "unset", overflowWrap: "anywhere" });
export const format = style({
  color: "#e1cfaa",
  fontSize: 16,
  textAlign: "end",
  marginInlineStart: "auto",
});
export const detail = style({
  gridColumn: 2,
  margin: 0,
  fontSize: 17,
  fontStyle: "italic",
  color: "#c2bbb1",
});
export const actions = style({
  gridColumn: "1 / -1",
  display: "flex",
  flexWrap: "wrap",
  justifyContent: "flex-end",
  gap: "6px 10px",
});
globalStyle(`${actions} button`, {
  font: '400 18px/1 Georgia,"Times New Roman",serif',
  height: 34,
  width: "min(108px, 100%)",
  borderRadius: 10,
  border: "1px solid #4b4946",
  background: "linear-gradient(130deg,#292829,#242424)",
  color: "#eeeae4",
  boxShadow: "inset 0 0 0 1px #ffffff05",
});
globalStyle(`${actions} button:first-child:enabled`, {
  borderColor: "#e4bc62",
  color: "#eed083",
  boxShadow: "inset 0 0 0 1px #c49a4833",
});
globalStyle(
  `${card}[data-kind="friend"] ${actions} button:first-child, ${card}[data-kind="trade"] ${actions} button:first-child`,
  { width: "min(117px, 100%)" },
);
globalStyle(`${actions} button:disabled`, {
  color: "#a7a6a1",
  background: "#323131",
});

globalStyle(
  `${card}[data-kind="friend"] ${heading}, ${card}[data-resolved=true] ${heading}`,
  {
    fontSize: 19,
    paddingInlineEnd: 0,
  },
);
globalStyle(`${card}[data-kind="friend"] ${heading} h3`, {
  letterSpacing: "-.28px",
});
globalStyle(`${card}[data-blocked=true] ${format}`, { color: "#b5b1aa" });
globalStyle(`${actions} button:enabled`, {
  boxShadow: "inset 0 0 0 1px #ffffff08,0 1px 1px #171717",
});
globalStyle(`${actions} button:first-child:enabled`, {
  boxShadow: "inset 0 0 0 1px #a0793b66,0 0 0 1px #161513,0 1px 3px #19150d",
});
globalStyle(`${actions} button:disabled`, {
  opacity: 0.8,
  borderColor: "#54514f",
});
