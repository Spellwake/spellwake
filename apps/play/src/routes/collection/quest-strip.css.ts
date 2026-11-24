import { style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";

const phone = ':root[data-device="phone"] &';

const compact = ':root[data-shell="compact"] &';
const wide = ':root:not([data-shell="compact"]) &';

/** Concept: today's quests in one slim row under the decks. */
export const strip = style({
  flexShrink: 0,
  display: "flex",
  alignItems: "stretch",
  gap: "16px",
  selectors: {
    // Along the bottom edge of a wide screen, with a rule between it and the decks.
    '&[data-place="edge"]': {
      padding: "14px 0 18px",
      borderTop: `1px solid ${t.color.fieldEdge}`,
    },
    [`${compact}[data-place="edge"]`]: { display: "none" },
    // At the end of the deck list, clear of the dock that floats over it.
    '&[data-place="list"]': { margin: "0 0 150px" },
    [`${wide}[data-place="list"]`]: { display: "none" },
    [phone]: { flexDirection: "column", gap: "8px" },
  },
});
export const heading = style({
  flexShrink: 0,
  alignSelf: "center",
  margin: 0,
  maxWidth: "110px",
  fontSize: t.font.hint,
  fontWeight: 400,
  lineHeight: 1.15,
  letterSpacing: "0.08em",
  textTransform: "uppercase",
  color: t.color.muted,
  selectors: { [phone]: { alignSelf: "flex-start", maxWidth: "none" } },
});
export const list = style({
  flex: 1,
  minWidth: 0,
  display: "grid",
  gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
  gap: "12px",
  margin: 0,
  padding: 0,
  listStyle: "none",
  selectors: {
    // A phone shows one quest and a sliver of the next; the row scrolls sideways.
    [phone]: {
      display: "flex",
      overflowX: "auto",
      scrollSnapType: "x mandatory",
      scrollbarWidth: "none",
      marginInline: "-16px",
      paddingInline: "16px",
    },
  },
});
export const quest = style({
  boxSizing: "border-box",
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  gap: "8px",
  padding: "10px 14px 10px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.fieldEdge}`,
  background: `color-mix(in srgb, ${t.color.field} 82%, transparent)`,
  color: t.color.ink,
  selectors: {
    '&[data-done="true"]': { borderColor: t.color.goldEdge },
    '&[aria-busy="true"]': { opacity: 0.6 },
    [phone]: { flex: "0 0 78%", scrollSnapAlign: "start" },
  },
});
export const top = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "8px",
  minHeight: "30px",
});
export const text = style({
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontSize: t.font.small,
  lineHeight: 1.2,
});
export const swap = style({
  appearance: "none",
  flexShrink: 0,
  display: "grid",
  placeItems: "center",
  width: "30px",
  height: "30px",
  padding: 0,
  borderRadius: "50%",
  border: `1px solid ${t.color.fieldEdge}`,
  background: "none",
  color: t.color.muted,
  cursor: "pointer",
  selectors: {
    "&:hover:not(:disabled)": {
      color: t.color.gold,
      borderColor: t.color.goldEdge,
    },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
    "&:disabled": { cursor: "default" },
  },
  "@media": {
    [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET },
  },
});
export const track = style({
  height: "4px",
  borderRadius: "2px",
  background: t.color.fieldEdge,
  overflow: "hidden",
});
export const fill = style({
  display: "block",
  blockSize: "100%",
  borderRadius: "inherit",
  background: t.color.gold,
});
export const bottom = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "8px",
  fontSize: t.font.hint,
  lineHeight: 1.2,
});
export const count = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "6px",
  color: t.color.muted,
  fontVariantNumeric: "tabular-nums",
  selectors: { [`${quest}[data-done="true"] &`]: { color: t.color.gold } },
});
export const check = style({ width: "1em", height: "1em" });
export const reward = style({ display: "inline-flex", color: t.color.gold });

/** Games won today and what they have paid. */
export const wins = style({
  flexShrink: 0,
  alignSelf: "center",
  display: "flex",
  alignItems: "baseline",
  gap: "8px",
  margin: 0,
  fontSize: t.font.hint,
  color: t.color.muted,
  selectors: { [phone]: { alignSelf: "flex-start" } },
});
export const winsCount = style({
  fontSize: "34px",
  lineHeight: 1,
  color: t.color.ink,
  fontVariantNumeric: "tabular-nums",
});
export const winsLabel = style({ whiteSpace: "nowrap" });
