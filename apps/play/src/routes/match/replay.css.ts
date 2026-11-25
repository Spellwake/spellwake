import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";

const landscapePhone =
  ':root[data-shell="compact"][data-orientation="landscape"] &';

/** A replay's controls, where the turn's actions sit during a game. */
export const panel = style({
  alignSelf: "stretch",
  boxSizing: "border-box",
  display: "flex",
  flexDirection: "column",
  gap: "12px",
  padding: "14px 16px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.goldEdge}`,
  background: `color-mix(in srgb, ${t.color.menu} 94%, transparent)`,
  color: t.color.ink,
  selectors: {
    // A sideways phone is short: the panel scrolls rather than running off it.
    [landscapePhone]: {
      gap: "8px",
      padding: "10px 12px",
      maxHeight: "calc(100dvh - 28px - env(safe-area-inset-top, 0px))",
      overflowY: "auto",
      scrollbarWidth: "thin",
    },
  },
});
// Each part keeps its height; a short screen scrolls the panel instead.
globalStyle(`${panel} > *`, { flexShrink: 0 });
// The shared chips are sized for a pointer. On a touch screen each is a fingertip tall.
globalStyle(`${panel} label > span`, {
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const title = style({
  margin: 0,
  fontSize: t.font.small,
  lineHeight: 1.2,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const kicker = style({
  fontSize: t.font.hint,
  letterSpacing: "0.08em",
  textTransform: "uppercase",
  color: t.color.muted,
});
export const scrubber = style({
  width: "100%",
  margin: 0,
  accentColor: t.color.gold,
  minHeight: "28px",
  cursor: "pointer",
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const buttons = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "6px",
});
const round = {
  appearance: "none",
  display: "grid",
  placeItems: "center",
  padding: 0,
  borderRadius: "50%",
  background: "none",
  cursor: "pointer",
} as const;
export const button = style({
  ...round,
  width: "max(40px, 24PX)",
  height: "max(40px, 24PX)",
  border: `1px solid ${t.color.fieldEdge}`,
  color: t.color.ink,
  selectors: {
    "&:hover:not(:disabled)": {
      borderColor: t.color.goldEdge,
      color: t.color.gold,
    },
    "&:disabled": { cursor: "default", opacity: 0.4 },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
  },
  "@media": { [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET } },
});
export const play = style([
  button,
  {
    width: "max(52px, 24PX)",
    height: "max(52px, 24PX)",
    borderColor: t.color.goldEdge,
    color: t.color.gold,
    "@media": {
      [touch]: {
        width: `max(52px, ${TOUCH_TARGET})`,
        height: `max(52px, ${TOUCH_TARGET})`,
      },
    },
  },
]);
export const count = style({
  margin: 0,
  textAlign: "center",
  fontSize: t.font.hint,
  color: t.color.muted,
  fontVariantNumeric: "tabular-nums",
});
export const setting = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "12px",
  fontSize: t.font.hint,
});
export const notice = style({
  margin: 0,
  fontSize: t.font.hint,
  color: t.color.gold,
});
/** The table's place while the game is read. */
export const reading = style({
  position: "absolute",
  inset: 0,
  display: "grid",
  placeItems: "center",
});
export const empty = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  padding: 24,
});
