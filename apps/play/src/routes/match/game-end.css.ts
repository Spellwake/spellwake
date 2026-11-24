import { style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";

const portraitPhone =
  ':root[data-shell="compact"][data-orientation="portrait"] &';
const landscapePhone =
  ':root[data-shell="compact"][data-orientation="landscape"] &';
// Only a win is gold. Every other ending reads in the table's plain ink.
const notWon = '&:not([data-outcome="won"])';

/** The result where the turn's actions were, once the band has been lifted. */
export const panel = style({
  alignSelf: "stretch",
  boxSizing: "border-box",
  padding: "16px 18px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.goldEdge}`,
  background: `color-mix(in srgb, ${t.color.menu} 94%, transparent)`,
  display: "flex",
  flexDirection: "column",
  gap: "10px",
  textAlign: "center",
  color: t.color.gold,
  selectors: {
    [notWon]: { borderColor: t.color.fieldEdge, color: t.color.ink },
    // It takes the keyboard so the result is read out; it is not a control.
    "&:focus": { outline: "none" },
  },
});
export const panelTitle = style({
  margin: 0,
  fontSize: "34px",
  lineHeight: 1.1,
  selectors: { [landscapePhone]: { fontSize: "26px" } },
});
export const reason = style({
  margin: 0,
  fontSize: t.font.hint,
  lineHeight: 1.25,
  color: t.color.muted,
});

/** The table dims behind the band so the result reads first. */
export const scrim = style({
  position: "absolute",
  inset: 0,
  zIndex: 1,
  background: "#06080bb8",
});
/** A band across the middle of the table. */
export const band = style({
  position: "absolute",
  zIndex: 1,
  insetInline: 0,
  top: "50%",
  transform: "translateY(-50%)",
  boxSizing: "border-box",
  padding: "34px 24px 30px",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "12px",
  textAlign: "center",
  color: t.color.gold,
  selectors: {
    [notWon]: { color: t.color.ink },
    "&:focus": { outline: "none" },
    [portraitPhone]: { top: "38%", padding: "26px 16px 22px" },
    [landscapePhone]: { padding: "16px 16px 14px", gap: "8px" },
  },
});
/** The band's own surface. It opens from the middle; what it holds does not stretch with it. */
export const bandSurface = style({
  position: "absolute",
  inset: 0,
  zIndex: -1,
  borderBlock: `1px solid ${t.color.goldEdge}`,
  background: "linear-gradient(180deg, #171a20fa, #101319fc)",
  boxShadow: "0 18px 46px #000a",
  selectors: {
    [`${band}:not([data-outcome="won"]) &`]: {
      borderBlockColor: t.color.fieldEdge,
    },
  },
});
export const bandTitle = style({
  margin: 0,
  fontSize: "76px",
  lineHeight: 1,
  fontWeight: 400,
  letterSpacing: "0.04em",
  // Wide letters as it arrives must not wrap or push the band's edges.
  whiteSpace: "nowrap",
  selectors: {
    [`${band}[data-outcome="won"] &`]: { textShadow: "0 2px 14px #f3e2b733" },
    [portraitPhone]: { fontSize: "48px" },
    [landscapePhone]: { fontSize: "38px" },
  },
});
export const bandRule = style({
  width: "min(360px, 60%)",
  height: "1px",
  background: `linear-gradient(90deg, transparent, ${t.color.goldEdge}, transparent)`,
  selectors: {
    [`${band}:not([data-outcome="won"]) &`]: {
      background: `linear-gradient(90deg, transparent, ${t.color.fieldEdge}, transparent)`,
    },
  },
});
export const bandActions = style({
  display: "flex",
  flexWrap: "wrap",
  justifyContent: "center",
  gap: "12px",
  marginTop: "8px",
});
