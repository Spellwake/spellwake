import {
  createVar,
  fallbackVar,
  globalStyle,
  style,
} from "@vanilla-extract/css";
import {
  standardMotion,
  feedbackTransition,
  settleTransition,
} from "../control-motion";
import { controlTokens as t } from "../controls.css";

const p = t.preference;
const progress = "var(--sw-range-progress)";
const thickness = createVar();
const diameter = createVar();
const trackColor = createVar();
const halo = createVar();

export const rangeWrap = style({
  display: "flex",
  alignItems: "center",
  width: "100%",
  minWidth: 0,
  gap: p.adjustGap,
  vars: {
    [thickness]: p.railHeight,
    [diameter]: p.railThumb,
    [trackColor]: p.rail,
  },
});
export const adjusted = style({
  vars: {
    [thickness]: p.cropHeight,
    [diameter]: p.cropThumb,
    [trackColor]: p.cropRail,
  },
});
const focus = {
  outline: `${t.ring.line} solid ${t.color.focus}`,
  outlineOffset: t.ring.offset,
};
export const range = style({
  appearance: "none",
  WebkitAppearance: "none",
  flex: 1,
  width: "100%",
  minWidth: 0,
  height: p.sliderHit,
  margin: 0,
  padding: 0,
  border: 0,
  background: "transparent",
  cursor: "pointer",
  selectors: {
    "&:focus": {
      outline: "none",
      vars: {
        [halo]: `0 0 0 ${p.focusGap} #2c2923, 0 0 0 calc(${p.focusGap} + ${t.ring.line}) ${t.color.goldEdge}`,
      },
    },
    "&:disabled": { opacity: 0.5, cursor: "default" },
  },
  "@media": {
    // The rail stays as drawn. The band that takes a touch grows around it.
    "(pointer: coarse)": { height: "44PX" },
    "(forced-colors: active)": {
      appearance: "auto",
      WebkitAppearance: "revert",
      selectors: { "&:focus-visible": focus },
    },
  },
});
const track = {
  height: thickness,
  borderRadius: p.pill,
  background: `linear-gradient(to right, ${p.fill} ${progress}, ${trackColor} ${progress})`,
};
const thumb = {
  boxSizing: "border-box",
  width: diameter,
  height: diameter,
  borderRadius: "50%",
  background: p.thumb,
  border: `${p.thin} solid ${p.thumbEdge}`,
  boxShadow: fallbackVar(halo, p.thumbHalo),
  "@media": {
    [standardMotion]: { transition: `transform ${feedbackTransition}` },
  },
} as const;
globalStyle(`${range}::-webkit-slider-runnable-track`, track);
globalStyle(`${range}::-moz-range-track`, track);
globalStyle(`${range}::-webkit-slider-thumb`, {
  ...thumb,
  WebkitAppearance: "none",
  marginTop: `calc((${thickness} - ${diameter}) / 2)`,
});
globalStyle(`${range}::-moz-range-thumb`, thumb);
for (const thumbSelector of ["::-webkit-slider-thumb", "::-moz-range-thumb"]) {
  globalStyle(`${range}:enabled:active${thumbSelector}`, {
    "@media": { [standardMotion]: { transform: "scale(1.1)" } },
  });
}
export const stepButton = style({
  display: "grid",
  placeItems: "center",
  flexShrink: 0,
  padding: 0,
  width: p.adjustButton,
  height: p.adjustButton,
  border: `${p.thin} solid ${p.cropEdge}`,
  borderRadius: p.adjustRadius,
  background: p.cropSurface,
  color: p.optionInk,
  cursor: "pointer",
  "@media": {
    [standardMotion]: {
      transition: `transform ${feedbackTransition}, background-color ${feedbackTransition}`,
      selectors: { "&:enabled:active": { transform: "scale(.96)" } },
    },
    // A fingertip needs the whole button, not the designed 27 pixels.
    "(pointer: coarse)": { width: "44PX", height: "44PX" },
  },
  selectors: {
    "&:enabled:hover": { background: t.color.hover },
    "&:focus-visible": focus,
    "&:disabled": { opacity: 0.5, cursor: "default" },
  },
});
/**
 * The switch's pressable area. It is a label, so the whole of it toggles: the
 * drawn switch keeps its size while the area meets the minimum target, which
 * is larger for a fingertip.
 */
export const switchBox = style({
  position: "relative",
  display: "inline-flex",
  alignItems: "center",
  flexShrink: 0,
  width: p.switchWidth,
  height: p.switchHeight,
  minHeight: "24PX",
  cursor: "pointer",
  "@media": {
    "(pointer: coarse)": { minWidth: "44PX", minHeight: "44PX" },
  },
});
export const toggle = style({
  appearance: "none",
  WebkitAppearance: "none",
  boxSizing: "border-box",
  width: "100%",
  height: p.switchHeight,
  margin: 0,
  borderRadius: p.pill,
  border: `${p.thin} solid ${p.offEdge}`,
  backgroundColor: p.off,
  cursor: "pointer",
  selectors: {
    "&:checked": { backgroundColor: p.on, borderColor: p.thumbEdge },
    "&:focus-visible": focus,
    "&:disabled": { opacity: 0.5, cursor: "default" },
  },
  "@media": {
    [standardMotion]: {
      transition: `background-color ${settleTransition}, border-color ${settleTransition}`,
    },
    "(forced-colors: active)": {
      appearance: "auto",
      WebkitAppearance: "revert",
    },
  },
});
const inset = `calc(${p.switchInset} + ${p.thin})`;
// One carrier width is exactly the thumb's travel, including at the spectator review scale.
export const thumbTravel = style({
  position: "absolute",
  top: 0,
  bottom: 0,
  left: inset,
  right: `calc(${inset} + ${p.switchThumb})`,
  display: "flex",
  alignItems: "center",
  pointerEvents: "none",
  selectors: {
    "&::before": {
      content: '""',
      flexShrink: 0,
      width: p.switchThumb,
      height: p.switchThumb,
      borderRadius: "50%",
      backgroundColor: p.offThumb,
    },
    [`${toggle}:checked + &::before`]: { backgroundColor: p.fill },
    [`${toggle}:disabled + &`]: { opacity: 0.5 },
  },
  "@media": {
    [standardMotion]: {
      selectors: {
        "&::before": { transition: `background-color ${settleTransition}` },
      },
    },
    // Keep the native state visible immediately, including when the OS preference changes during travel.
    "(prefers-reduced-motion: reduce)": {
      transform: "none !important",
      selectors: {
        [`${toggle}:checked + &`]: { transform: "translateX(100%) !important" },
      },
    },
    "(forced-colors: active)": { display: "none" },
  },
});
export const options = style({
  display: "grid",
  gap: p.optionGap,
  padding: 0,
  margin: 0,
  border: 0,
  minWidth: 0,
});
export const optionLabel = style({
  display: "flex",
  alignItems: "center",
  gap: p.radioGap,
  width: "fit-content",
  fontFamily: t.font.face,
  fontSize: t.font.size,
  lineHeight: 1.1,
  color: p.optionInk,
  cursor: "pointer",
  WebkitFontSmoothing: "antialiased",
  // A pointer target never drops below 24 real pixels, however small the display scale.
  minHeight: "24PX",
  selectors: { "&:has(input:disabled)": { opacity: 0.5, cursor: "default" } },
  // The whole row is the target, so a fingertip does not have to find the ring.
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
export const radio = style({
  appearance: "none",
  WebkitAppearance: "none",
  boxSizing: "border-box",
  display: "grid",
  placeContent: "center",
  flexShrink: 0,
  margin: 0,
  width: p.radioSize,
  height: p.radioSize,
  border: `${t.ring.line} solid ${t.color.goldEdge}`,
  borderRadius: "50%",
  cursor: "inherit",
  background: p.cropSurface,
  selectors: {
    "&::before": {
      opacity: 0,
      transform: "scale(.65)",
      content: '""',
      width: p.radioDot,
      height: p.radioDot,
      borderRadius: "50%",
      background: p.fill,
    },
    "&:checked::before": { opacity: 1, transform: "scale(1)" },
    "&:focus-visible": focus,
  },
  "@media": {
    [standardMotion]: {
      selectors: {
        "&::before": {
          transition: `opacity ${feedbackTransition}, transform ${feedbackTransition}`,
        },
      },
    },
    "(forced-colors: active)": {
      appearance: "auto",
      WebkitAppearance: "revert",
      selectors: { "&::before": { display: "none" } },
    },
  },
});

export const chips = style([
  options,
  {
    display: "flex",
    flexWrap: "wrap",
    gap: 10,
  },
]);
export const chipLabel = style({
  position: "relative",
  flex: "1 1 max-content",
  minWidth: 0,
  maxWidth: "100%",
  cursor: "pointer",
});
export const chip = style({
  position: "relative",
  isolation: "isolate",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 8,
  boxSizing: "border-box",
  minHeight: 37,
  padding: "3px 11px",
  borderRadius: 22,
  border: "1px solid #aa936e",
  background: "linear-gradient(115deg,#232a22ef,#272c23ed)",
  color: "#f4e8cd",
  font: '400 .96rem/1.14 "Albert Sans",system-ui,sans-serif',
  WebkitFontSmoothing: "antialiased",
  selectors: {
    "&::before": {
      content: '""',
      inset: 0,
      position: "absolute",
      zIndex: -1,
      borderRadius: "inherit",
      opacity: 0,
      background: "linear-gradient(110deg,#f3dba7,#ffecc4 48%,#eace96)",
    },
    "input:checked + &": {
      color: "#282015",
      borderColor: "#f5dba4",
      boxShadow: "0 0 6px #eccb8840",
    },
    "input:checked + &::before": { opacity: 1 },
    "input:focus-visible + &": {
      outline: "2px solid #fff1c9",
      outlineOffset: 2,
    },
    "input:enabled:not(:checked) + &:hover": { borderColor: "#e9d4aa" },
    "input:disabled + &": { opacity: 0.6, cursor: "default" },
  },
  "@media": {
    [standardMotion]: {
      transition: `color ${feedbackTransition}, border-color ${feedbackTransition}, box-shadow ${settleTransition}`,
      selectors: { "&::before": { transition: `opacity ${settleTransition}` } },
    },
    "(forced-colors: active)": {
      selectors: {
        "input:checked + &": {
          outline: "2px solid Highlight",
          outlineOffset: -3,
        },
      },
    },
  },
});
