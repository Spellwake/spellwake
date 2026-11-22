import { globalStyle, style } from "@vanilla-extract/css";
import * as picker from "../inventory/picker.css";
import * as nav from "../navigation/navigation.css";
import * as controls from "../controls.css";
import { feedbackTransition, standardMotion } from "../control-motion";

const shortLandscape = "(max-height: 480px) and (min-width: 659px)";
export const dialog = style({
  boxSizing: "border-box",
  width: "min(658px, 100vw)",
  height: "min(706px, 100dvh)",
  maxWidth: "100vw",
  maxHeight: "100dvh",
  padding: 0,
  margin: "auto",
  border: 0,
  background: "transparent",
  color: "#ede9dc",
  "@media": {
    [shortLandscape]: { width: "min(726px,100vw)" },
  },
  overflow: "auto",
  overscrollBehavior: "contain",
  selectors: {
    "&::backdrop": {
      background: "#03060ac9",
      overflow: "hidden",
      overscrollBehavior: "contain",
    },
  },
});
export const frame = style({
  height: "100%",
  minHeight: 290,
  boxSizing: "border-box",
  display: "grid",
  gridTemplateRows: "76px minmax(0,1fr)",
  border: "2px solid #afa68f",
  borderRadius: 20,
  padding: "0 clamp(12px,4.2vw,27px)",
  background: "linear-gradient(160deg,#15191b,#0b1012 26%,#090e10)",
  font: '24px/1.15 Baskerville,"Times New Roman",serif',
});
export const header = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "0 0 3px 13px",
});
globalStyle(`${header} h2`, {
  margin: 0,
  outline: "none",
  font: '42px/1.1 Baskerville,"Times New Roman",serif',
  color: "#f4f1e7",
});
export const close = style({
  selectors: {
    "&&": {
      width: 36,
      minWidth: 0,
      height: 36,
      padding: 0,
      justifyContent: "center",
      border: "none",
      background: "transparent",
      boxShadow: "none",
      color: "#f5f0e6",
    },
  },
});
globalStyle(`${frame} ${nav.tabRoot}`, {
  display: "grid",
  minHeight: 0,
  gridTemplateRows: "auto minmax(0,1fr)",
  WebkitTextStroke: "0",
  font: "24px/1.15 Baskerville,serif",
});
globalStyle(`${frame} ${nav.viewport}`, { marginInline: 11, paddingBottom: 0 });
globalStyle(`${frame} ${nav.strip}`, {
  gap: 13,
  border: 0,
  alignItems: "center",
});
globalStyle(`${frame} ${nav.strip}::after`, {
  content: '""',
  height: 1,
  alignSelf: "end",
  marginBottom: 4,
  background: "#5e5e56",
  flex: 1,
});
globalStyle(`${frame} ${nav.trigger}`, {
  minWidth: 0,
  width: "min(143px,25vw)",
  height: 42,
  padding: "0 8px",
  borderRadius: 10,
  border: "1px solid #867a68",
  color: "#eee7d9",
  font: "23px/1.1 Baskerville,serif",
  background: "linear-gradient(#171b1c,#0b1113)",
});
globalStyle(`${frame} ${nav.trigger}[aria-selected=true]`, {
  color: "#fff6dd",
  borderColor: "#d0bfa1",
  background: "linear-gradient(125deg,#4b4336,#352f27)",
});
globalStyle(`${frame} ${nav.underline}`, { display: "none" });
globalStyle(`${frame} [role=tabpanel][data-state=active]`, {
  display: "grid",
  minHeight: 0,
  margin: 0,
  padding: "17px 0 0",
  gridTemplateRows: "auto minmax(0,1fr) 58px",
  rowGap: 16,
});
export const search = style({ marginInline: 11 });
globalStyle(`${search} ${controls.frame}`, {
  height: 43,
  minHeight: 0,
  border: "1px solid #867a68",
  borderRadius: 22,
  paddingInline: 22,
  gap: 19,
  background: "#0c1113",
  boxShadow: "inset 0 1px 3px #0009",
});
globalStyle(`${search} input`, { font: "italic 22px/1.15 Baskerville,serif" });
export const list = style({
  minHeight: 0,
  overflowY: "auto",
  overscrollBehaviorY: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#c9b293 #232626",
  alignSelf: "start",
  maxHeight: "100%",
  boxSizing: "border-box",
  border: "1px solid #595952",
  borderRadius: 10,
  paddingBlock: 3,
  selectors: { "&[data-empty]": { alignSelf: "stretch", border: 0 } },
});
export const empty = style({
  selectors: { "&:empty": { height: 0, overflow: "hidden" } },
  height: "100%",
  display: "grid",
  placeItems: "center",
  padding: 0,
  margin: 0,
  font: "italic 25px/1.2 Baskerville,serif",
  color: "#e5dcca",
});
export const total = style({
  marginInline: 11,
  padding: "10px 7px 0",
  borderTop: "1px solid #b9b3a6",
  selectors: { "&:empty": { borderTop: 0 } },
  color: "#ede7d7",
  font: "italic 23px/1.25 Baskerville,serif",
});
globalStyle(`${frame} ${nav.trigger}`, {
  "@media": {
    [standardMotion]: {
      transition: `background-color ${feedbackTransition},color ${feedbackTransition}`,
    },
  },
});

globalStyle(`${search} input::placeholder`, { color: "#e0cfaf", opacity: 1 });
globalStyle(`${list} ${picker.picker}[data-mode=add]`, {
  border: 0,
  borderRadius: 0,
});
globalStyle(
  `${list}[data-kind=card] ${picker.picker}[data-mode=add] ${picker.row}`,
  { minHeight: 71, paddingBlock: 4 },
);
globalStyle(
  `${list}[data-kind=pack] ${picker.picker}[data-mode=add] ${picker.row}`,
  { minHeight: 97 },
);
globalStyle(`${list} ${picker.picker}[data-mode=add] ${picker.detail}`, {
  fontSize: 18,
  color: "#ebe1cc",
});

globalStyle(`${frame} ${nav.viewport}, ${frame} ${search}`, {
  "@media": { [shortLandscape]: { marginInline: 3 } },
});
globalStyle(`${frame} ${nav.strip}::after`, {
  "@media": { [shortLandscape]: { display: "none" } },
});
globalStyle(`${frame} ${nav.trigger}`, {
  "@media": { [shortLandscape]: { width: 151, height: 46, fontSize: 26 } },
});
globalStyle(`${search} ${controls.frame}`, {
  "@media": {
    [shortLandscape]: { height: 50, borderRadius: 13, borderColor: "#b9b09b" },
  },
});
globalStyle(`${search} input`, {
  "@media": { [shortLandscape]: { font: "italic 26px/1.15 Georgia,serif" } },
});

globalStyle(`${frame} [role=tabpanel]:has(${list}[data-kind=coin])`, {
  gridTemplateRows: "minmax(0,1fr) 58px",
});
// Close, the category tabs and the search field never drop below a fingertip on touch screens.
globalStyle(`${close}${close}`, {
  "@media": { "(pointer: coarse)": { width: "44PX", height: "44PX" } },
});
globalStyle(`${frame} ${nav.trigger}, ${search} ${controls.frame}`, {
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
