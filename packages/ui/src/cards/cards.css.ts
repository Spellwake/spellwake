import { style } from "@vanilla-extract/css";
import { standardMotion, feedbackTransition } from "../control-motion";
export const face = style({
  display: "block",
  position: "relative",
  width: "100%",
  isolation: "isolate",
});
export const scan = style({
  display: "block",
  width: "100%",
  height: "100%",
  borderRadius: "4% / 3%",
  boxShadow: "0 2px 5px #070a08b0",
});
export const choices = style({ display: "flex", alignItems: "start" });
export const option = style({
  width: "var(--face-width)",
  flex: "none",
  textAlign: "center",
});
export const reason = style({
  display: "block",
  marginTop: 3,
  font: 'italic 18px/1.15 "Iowan Old Style",Georgia,serif',
  color: "#d1c8ac",
  textShadow: "0 1px 2px #000",
});
export const mark = style({
  position: "absolute",
  boxSizing: "border-box",
  display: "block",
  padding: 0,
  margin: 0,
  border: "2px solid",
  borderRadius: 9,
  pointerEvents: "none",
  appearance: "none",
  selectors: {
    '&[data-tone="available"]': {
      background: "rgb(39 240 229 / 10%)",
      borderColor: "#26e2de",
      boxShadow: "0 0 3px #35dfdb85",
    },
    '&[data-tone="selected"]': {
      background: "rgb(255 198 37 / 13%)",
      borderColor: "#efb724",
      boxShadow: "0 0 3px #eeb52e90",
    },
    '&[data-tone="muted"]': {
      background: "rgb(73 75 77 / 6%)",
      borderColor: "#737a82b5",
      borderWidth: 2,
    },
    "button&": { cursor: "pointer", pointerEvents: "auto" },
    "button&:disabled": { cursor: "default" },
    "button&:focus-visible": { outline: "2px solid #fff5c9", outlineOffset: 2 },
    "button&:hover:not(:disabled)": {
      boxShadow: "0 0 0 1px currentColor,0 0 6px #fff6",
      color: "#dffcf4",
    },
  },
  "@media": {
    [standardMotion]: {
      transition: `border-color ${feedbackTransition},background-color ${feedbackTransition},box-shadow ${feedbackTransition}`,
    },
  },
});
export const tick = style({
  position: "absolute",
  left: "100%",
  top: "50%",
  translate: "0 -50%",
  width: 20,
  height: 20,
  color: "#ffc42d",
  filter: "drop-shadow(0 1px 1px #493600)",
  pointerEvents: "none",
});
