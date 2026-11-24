import { style } from "@vanilla-extract/css";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * League · History. It sits in the League page's own container and table
 * panel; these are its columns and the stacked row of a narrow page.
 */
const NARROW = "league (inline-size < 46rem)";
const gold = "#d9b878";
const rule = "#a698774c";
const muted = "#b9ae99";
const columns =
  "minmax(0,0.8fr) minmax(0,2.2fr) minmax(0,1.5fr) minmax(0,0.7fr) minmax(0,0.9fr)";

export const head = style({
  flexShrink: 0,
  display: "grid",
  gridTemplateColumns: columns,
  alignItems: "center",
  gap: "16px",
  padding: "14px 22px",
  borderBottom: `1px solid ${rule}`,
  font: `21px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": {
    // Stacked rows name their own facts.
    [NARROW]: {
      position: "absolute",
      width: "1px",
      height: "1px",
      overflow: "hidden",
      clip: "rect(0 0 0 0)",
      padding: 0,
    },
  },
});
export const row = style({
  display: "grid",
  gridTemplateColumns: columns,
  alignItems: "center",
  gap: "16px",
  minHeight: "78px",
  padding: "6px 22px",
  borderBottom: `1px solid ${rule}`,
  font: `26px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  selectors: { "&:last-child": { borderBottom: 0 } },
  "@container": {
    [NARROW]: {
      gridTemplateColumns: "minmax(0,1fr) auto",
      gridTemplateAreas: '"opponent outcome" "format played"',
      gap: "4px 12px",
      minHeight: "64px",
      padding: "12px 8px",
      fontSize: "21px",
    },
  },
});
/** Only a win is gold, as at the table. */
export const outcome = style({
  selectors: { [`${row}[data-outcome="won"] &`]: { color: gold } },
  "@container": {
    [NARROW]: { gridArea: "outcome", textAlign: "right" },
  },
});
/** The score beside the result, where the Games column is gone. */
export const outcomeGames = style({
  display: "none",
  fontVariantNumeric: "tabular-nums",
  "@container": { [NARROW]: { display: "inline" } },
});
export const opponent = style({
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  "@container": { [NARROW]: { gridArea: "opponent" } },
});
/** "vs" reads only where the heading above the column is gone. */
export const versus = style({
  display: "none",
  color: muted,
  "@container": { [NARROW]: { display: "inline" } },
});
export const format = style({
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  "@container": {
    [NARROW]: { gridArea: "format", fontSize: "17px", color: muted },
  },
});
export const ranked = style({ color: muted });
export const games = style({
  textAlign: "center",
  fontVariantNumeric: "tabular-nums",
  // Narrow, the score joins the result: "Won 2–1" reads as one fact.
  "@container": { [NARROW]: { display: "none" } },
});
export const played = style({
  textAlign: "right",
  color: muted,
  whiteSpace: "nowrap",
  "@container": {
    [NARROW]: { gridArea: "played", fontSize: "17px" },
  },
});
/** A fact still being read: centred in its column, and one line on a narrow page. */
export const pendingFact = style({
  width: "60%",
  justifySelf: "center",
  "@container": { [NARROW]: { display: "none" } },
});
