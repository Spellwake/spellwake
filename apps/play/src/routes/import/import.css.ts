import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/**
 * Import deck, as approved (IMP-01 to IMP-05). Lengths are in the pixel scale
 * of the approved references. Every source tab keeps one height, so switching
 * tabs never moves the actions.
 */
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;
const fieldError = "#f0c674";

export const shell = style({
  selectors: { '&&:not([data-presentation="drawer"])': { width: "740px" } },
});
// The approved dialog centres its two actions.
globalStyle(`${shell} > footer`, { justifyContent: "center" });
export const body = style({
  position: "relative",
  display: "flex",
  flexDirection: "column",
  gap: "18px",
});
export const panel = style({
  display: "flex",
  flexDirection: "column",
  gap: "12px",
  // A fixed height: a field message takes its room from the list or drop
  // zone, so the dialog never grows when one appears.
  height: "330px",
});
export const label = style({
  font: `25px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const list = style({
  display: "block",
  width: "100%",
  flex: "1 1 0",
  minHeight: "120px",
  resize: "none",
  padding: "14px 22px",
  borderRadius: "10px",
  border: `1px solid ${t.color.formEdge}`,
  background: "#05090fb3",
  color: shellTokens.ink,
  font: `25px/1.15 ${shellTokens.serif}`,
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  selectors: {
    "&::placeholder": { color: "#8f8a7c" },
    "&:focus-visible": focus,
    '&[aria-invalid="true"]': { borderColor: fieldError },
  },
});
export const message = style({
  margin: 0,
  font: `22px/1.3 ${shellTokens.serif}`,
  color: fieldError,
});
export const hint = style({
  margin: 0,
  font: `21px/1.3 ${shellTokens.serif}`,
  color: "#b9b2a4",
});
/** Where a file is dropped or chosen. It lights when a file is held over it. */
export const drop = style({
  display: "grid",
  placeContent: "center",
  justifyItems: "center",
  gap: "10px",
  flex: "1 1 0",
  minHeight: "160px",
  padding: "24px",
  borderRadius: "10px",
  border: `1px solid ${t.color.formEdge}`,
  background: "#05090fb3",
  textAlign: "center",
  selectors: {
    '&[data-over="true"]': {
      borderColor: shellTokens.line,
      background: "#1a1a14cc",
      boxShadow: `inset 0 0 0 1px ${shellTokens.line}66`,
    },
    '&[aria-invalid="true"]': { borderColor: fieldError },
  },
  "@media": {
    [standardMotion]: {
      transition: `border-color ${m.duration.feedback}s ${cssEase(m.ease.arrive)}, background-color ${m.duration.feedback}s ${cssEase(m.ease.arrive)}`,
    },
  },
});
export const dropTitle = style({
  margin: 0,
  font: `32px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  overflowWrap: "anywhere",
});
export const dropTypes = style({
  margin: "0 0 8px",
  font: `23px/1.2 ${shellTokens.serif}`,
  color: "#d8cdb9",
});
/** The real file input, kept for the system chooser and never drawn. */
export const hiddenInput = style({
  position: "absolute",
  width: "1px",
  height: "1px",
  opacity: 0,
  pointerEvents: "none",
});
export const action = style({
  selectors: { "&&": { minHeight: "52px", padding: "0 44px" } },
  "@media": {
    [touch]: {
      selectors: { "&&": { minHeight: `max(52px, ${TOUCH_TARGET})` } },
    },
  },
});
/** One line of passive feedback, beside the actions. */
export const notice = style({
  position: "absolute",
  right: 0,
  bottom: "-6px",
  width: "min(100%, 420px)",
  pointerEvents: "none",
});
