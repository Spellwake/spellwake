import { style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";

const phone = ':root[data-device="phone"] &';

/** Sideboarding between games: the main deck and the sideboard side by side. */
export const body = style({
  display: "flex",
  flexDirection: "column",
  gap: "14px",
});
export const columns = style({
  display: "grid",
  gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
  gap: "16px",
  selectors: { [phone]: { gridTemplateColumns: "minmax(0, 1fr)" } },
});
export const column = style({
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  gap: "8px",
});
export const columnTitle = style({
  margin: 0,
  fontSize: t.font.small,
  fontWeight: 400,
  color: t.color.ink,
});
export const count = style({
  color: t.color.muted,
  fontVariantNumeric: "tabular-nums",
});
export const list = style({
  margin: 0,
  padding: 0,
  listStyle: "none",
  display: "flex",
  flexDirection: "column",
  gap: "4px",
  maxHeight: "48vh",
  overflowY: "auto",
  scrollbarWidth: "thin",
  selectors: { [phone]: { maxHeight: "none" } },
});
export const row = style({
  boxSizing: "border-box",
  display: "flex",
  alignItems: "center",
  gap: "10px",
  minHeight: "40px",
  padding: "2px 3px 2px 12px",
  textAlign: "start",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.fieldEdge}`,
  background: t.color.field,
  fontSize: t.font.hint,
  color: t.color.ink,
  // On a touch screen the move button is a fingertip tall; the row holds it.
  "@media": { [touch]: { minHeight: `calc(${TOUCH_TARGET} + 6px)` } },
});
export const quantity = style({
  flexShrink: 0,
  minWidth: "1.4em",
  textAlign: "right",
  color: t.color.gold,
  fontVariantNumeric: "tabular-nums",
});
export const name = style({
  flex: 1,
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const move = style({
  appearance: "none",
  flexShrink: 0,
  display: "grid",
  placeItems: "center",
  // Never smaller than a pointer can press, however the page is scaled.
  width: "max(32px, 24PX)",
  height: "max(32px, 24PX)",
  padding: 0,
  borderRadius: "50%",
  border: `1px solid ${t.color.fieldEdge}`,
  background: "none",
  color: t.color.muted,
  cursor: "pointer",
  selectors: {
    "&:hover": { color: t.color.gold, borderColor: t.color.goldEdge },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
  },
  "@media": { [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET } },
});
export const empty = style({
  margin: 0,
  padding: "8px 0",
  fontSize: t.font.hint,
  fontStyle: "italic",
  color: t.color.muted,
});
/** Why the list cannot be used yet, or why it did not go through. */
export const note = style({
  margin: 0,
  fontSize: t.font.hint,
  color: t.color.gold,
});
