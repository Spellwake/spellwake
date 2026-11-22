import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "../controls.css";

export const panel = style({
  boxSizing: "border-box",
  width: "100%",
  maxWidth: 705,
  padding: "clamp(18px,5%,31px) clamp(14px,5%,33px) clamp(18px,5%,29px)",
  border: "2px solid #c9bcaa",
  borderRadius: 21,
  background: "linear-gradient(124deg,#15181a,#121517 65%,#161819)",
  color: "#f5eadb",
  fontFamily: t.font.face,
  textShadow: "0 1px 2px #000a",
});
export const copy = style({ paddingInline: "min(2%,12px)" });
export const title = style({
  display: "block",
  width: "fit-content",
  margin: 0,
  font: "inherit",
  fontSize: 48,
  lineHeight: "1.16",
  overflowWrap: "anywhere",
});
globalStyle(`${panel}[data-reason=unavailable] ${title}`, {
  fontSize: 35,
  color: "#d1c7bb",
});
globalStyle(`${panel}[data-reason=unavailable]`, {
  paddingTop: "clamp(18px,5%,28px)",
});
export const detail = style({
  margin: "5px 0 0",
  fontSize: 32,
  lineHeight: "1.145",
  overflowWrap: "anywhere",
});
export const footer = style({
  marginTop: 25,
  borderTop: "1px solid #8c8173",
  padding: "27px min(2%,12px) 0",
});
export const back = style({
  selectors: {
    "&&": {
      minWidth: 0,
      width: "min(205px,100%)",
      height: 58,
      borderRadius: 12,
      border: "2px solid #d2c1a8",
      fontFamily: t.font.face,
      fontSize: 29,
      background: "linear-gradient(165deg,#262625,#181a1b 65%,#141617)",
      color: "#f3eadc",
      gap: 14,
      paddingInline: "6px 30px",
      boxShadow: "inset 0 0 0 1px #ffffff0d,0 1px 2px #0008",
    },
  },
});

globalStyle(`${back} span svg`, { width: 34, height: 34 });
// Back never drops below a fingertip on touch screens.
globalStyle(`${back}${back}`, {
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
