import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { cssEase, motionLanguage as m } from "../../lib/motion";

const phone = ':root[data-device="phone"] &';
/** A phone on its side has the width for one toolbar row and more columns. */
const phoneLandscape =
  ':root[data-device="phone"][data-orientation="landscape"] &';
const arrive = cssEase(m.ease.arrive);

/** Toolbar breakpoints, in design pixels as rem so they follow display density. */
const TOOLBAR_TWO_ROWS = "shelf (inline-size < 61rem)";
const TOOLBAR_THREE_ROWS = "shelf (inline-size < 35rem)";

export const page = style({
  // The shelf lays itself out by its own width, wherever the frame places it.
  containerName: "shelf",
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  padding: "30px 3.3% 0",
  selectors: {
    [phone]: { padding: "22px 22px 0" },
  },
});
/** Column proportions measured from the approved library reference. Narrower shelves wrap rather than squeeze. */
export const toolbar = style({
  position: "relative",
  zIndex: 2,
  flexShrink: 0,
  minHeight: "58px",
  display: "grid",
  gridTemplateColumns:
    "minmax(0,432fr) minmax(0,268fr) minmax(max-content,219fr) minmax(max-content,239fr)",
  alignItems: "center",
  gap: "36px",
  "@container": {
    [TOOLBAR_TWO_ROWS]: {
      gridTemplateColumns: "minmax(0,1fr) max-content max-content",
      gap: "14px 18px",
    },
    [TOOLBAR_THREE_ROWS]: {
      gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
      gap: "14px",
    },
  },
});
export const search = style({
  minWidth: 0,
  "@container": { [TOOLBAR_TWO_ROWS]: { gridColumn: "1 / -1" } },
});
export const format = style({ minWidth: 0 });
globalStyle(`${format} > button`, { width: "100%" });
globalStyle(`${toolbar} button`, { whiteSpace: "nowrap" });
globalStyle(`${toolbar} > button`, { width: "100%" });
export const formatWide = style({
  "@container": { [TOOLBAR_THREE_ROWS]: { gridColumn: "1 / -1" } },
});

export const scroll = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  marginInline: "-3.3%",
  paddingInline: "3.3%",
  // The fade lets tiles dissolve under the toolbar instead of clipping on a hard edge.
  WebkitMaskImage:
    "linear-gradient(180deg,transparent 0,#000 34px,#000 calc(100% - 28px),transparent 100%)",
  maskImage:
    "linear-gradient(180deg,transparent 0,#000 34px,#000 calc(100% - 28px),transparent 100%)",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "-2px",
    },
    // The dock floats over the last rows, so focused tiles scroll clear of it.
    [phone]: {
      marginInline: "-22px",
      paddingInline: "22px",
      scrollPaddingBottom: "140px",
    },
  },
});
/** Fixed tiles on a centred shelf: wider windows gain columns instead of stretching the gaps. */
export const grid = style({
  display: "grid",
  gridTemplateColumns: "repeat(auto-fill, 324px)",
  justifyContent: "center",
  gap: "20px 74px",
  padding: "38px 0 56px",
  selectors: {
    [phone]: {
      gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
      gap: "26px 18px",
      padding: "30px 0 150px",
    },
    [phoneLandscape]: {
      gridTemplateColumns: "repeat(auto-fill, minmax(190px, 1fr))",
      padding: "24px 0 40px",
    },
  },
});

export const deck = style({
  position: "relative",
  width: "100%",
  maxWidth: "324px",
  margin: "0 auto",
});
const pool = keyframes({
  "0%, 100%": { opacity: 0.55 },
  "50%": { opacity: 0.8 },
});
/** Warm light pooled on the stone under each box. It answers hover instead of the whole tile lifting. */
export const floor = style({
  position: "absolute",
  left: "50%",
  top: "214px",
  width: "270px",
  height: "74px",
  transform: "translateX(-50%)",
  borderRadius: "50%",
  background:
    "radial-gradient(ellipse at center,#ffb86636 0%,#ff9a3c1c 38%,transparent 70%)",
  opacity: 0.55,
  pointerEvents: "none",
  selectors: {
    [`${deck}:hover &, ${deck}:focus-within &`]: {
      opacity: 1,
      transform: "translateX(-50%) scale(1.14)",
    },
    [phone]: { top: "168px", width: "210px", height: "58px" },
  },
  "@media": {
    [standardMotion]: {
      animation: `${pool} 6.4s ease-in-out infinite`,
      transition: `opacity ${m.duration.scene}s ${arrive}, transform ${m.duration.scene}s ${arrive}`,
      selectors: {
        [`${deck}:hover &, ${deck}:focus-within &`]: { animation: "none" },
      },
    },
  },
});
export const open = style({
  appearance: "none",
  display: "block",
  width: "100%",
  padding: 0,
  border: 0,
  background: "transparent",
  color: "inherit",
  font: "inherit",
  textAlign: "left",
  cursor: "pointer",
  position: "relative",
  borderRadius: t.size.radius,
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "4px",
    },
  },
});
/** The renderer frames the box by stage width, so the stage overhangs the tile to reach the approved box size. */
export const boxStage = style({
  display: "block",
  position: "relative",
  width: "calc(100% + 32px)",
  marginInline: "-16px",
  height: "268px",
  pointerEvents: "none",
  selectors: {
    [`${deck}:hover &, ${deck}:focus-within &`]: {
      transform: "translateY(-6px)",
    },
    [phone]: {
      width: "calc(100% + 20px)",
      marginInline: "-10px",
      height: "206px",
    },
  },
  "@media": {
    [standardMotion]: {
      transition: `transform ${m.duration.scene}s ${arrive}`,
    },
  },
});
globalStyle(`${boxStage} canvas`, {
  display: "block",
  position: "absolute",
  inset: 0,
});
const shimmer = keyframes({
  "0%, 100%": { opacity: 0.28 },
  "50%": { opacity: 0.6 },
});
/** Stands in for a box while its model prepares. The words stay available to assistive technology. */
export const boxStatus = style({
  position: "absolute",
  left: "50%",
  top: "16px",
  width: "176px",
  height: "224px",
  transform: "translateX(-50%)",
  borderRadius: "14px 14px 18px 18px",
  background: "linear-gradient(160deg,#f6e9d31c,#f6e9d308 60%)",
  border: "1px solid #f6e9d31f",
  color: "transparent",
  overflow: "hidden",
  userSelect: "none",
  selectors: { [phone]: { width: "136px", height: "174px" } },
  "@media": {
    [standardMotion]: { animation: `${shimmer} 1.8s ease-in-out infinite` },
  },
});
export const caption = style({
  display: "flex",
  alignItems: "flex-end",
  justifyContent: "space-between",
  gap: "12px",
  minHeight: "64px",
  marginTop: "-10px",
  // Inset towards the box, so the name and Play sit with it, not at the tile's edges.
  padding: "0 28px",
  selectors: { [phone]: { padding: "0 6px", minHeight: "58px" } },
});
export const title = style({ minWidth: 0, flex: 1 });
export const name = style({
  margin: 0,
  font: "inherit",
  fontSize: "27px",
  lineHeight: 1.18,
  color: "#f6e9cf",
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
  textShadow: "0 2px 8px #000",
  selectors: { [phone]: { fontSize: "24px" } },
});
export const formatLine = style({
  margin: "3px 0 0",
  fontSize: "21px",
  color: t.color.muted,
  textShadow: "0 1px 6px #000",
  selectors: { [phone]: { fontSize: "19px" } },
});
export const play = style({
  flexShrink: 0,
  marginBottom: "6px",
});
globalStyle(`${play} > button`, {
  minHeight: 0,
  height: "38px",
  padding: "0 14px",
  fontSize: "19px",
});
globalStyle(`${play} > button svg`, { width: "1em", height: "1em" });
// Touch screens get a fingertip-sized Play target at any density.
globalStyle(`${play} > button`, {
  "@media": {
    "(pointer: coarse)": { minHeight: "44PX", minWidth: "44PX" },
  },
});
// Phones drop the label for room and keep a round target.
globalStyle(`:root[data-device="phone"] ${play} > button`, {
  height: "max(62px, 44PX)",
  width: "max(62px, 44PX)",
  padding: 0,
  fontSize: 0,
  gap: 0,
  borderRadius: "50%",
});
globalStyle(`:root[data-device="phone"] ${play} > button svg`, {
  width: "22px",
  height: "22px",
});

export const center = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  padding: "45px 0",
});
