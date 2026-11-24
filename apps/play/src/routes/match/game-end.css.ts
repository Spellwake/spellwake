import { style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";

const portraitPhone =
  ':root[data-shell="compact"][data-orientation="portrait"] &';
const landscapePhone =
  ':root[data-shell="compact"][data-orientation="landscape"] &';

/** The result where the turn's actions were: it says how the game came out and offers the way out. */
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
});
export const panelTitle = style({
  margin: 0,
  fontSize: "34px",
  lineHeight: 1.1,
  color: t.color.gold,
  selectors: { [landscapePhone]: { fontSize: "26px" } },
});
export const reason = style({
  margin: 0,
  fontSize: t.font.hint,
  lineHeight: 1.25,
  color: t.color.muted,
});

/** Every seat's final standing, inside the result dialog. */
export const standings = style({
  margin: 0,
  padding: 0,
  listStyle: "none",
  display: "flex",
  flexDirection: "column",
  gap: "8px",
});
export const standing = style({
  display: "flex",
  justifyContent: "space-between",
  alignItems: "baseline",
  gap: "16px",
  padding: "10px 14px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.fieldEdge}`,
  background: t.color.field,
  fontSize: t.font.small,
  color: t.color.ink,
  selectors: {
    '&[data-won="true"]': { borderColor: t.color.goldEdge },
  },
});
export const standingNote = style({
  fontSize: t.font.hint,
  color: t.color.muted,
  whiteSpace: "nowrap",
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
  boxSizing: "border-box",
  padding: "34px 24px 30px",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "12px",
  textAlign: "center",
  borderBlock: `1px solid ${t.color.goldEdge}`,
  background: "linear-gradient(180deg, #171a20fa, #101319fc)",
  boxShadow: "0 18px 46px #000a",
  selectors: {
    [portraitPhone]: { top: "38%", padding: "26px 16px 22px" },
    [landscapePhone]: { padding: "16px 16px 14px", gap: "8px" },
  },
});
export const bandTitle = style({
  margin: 0,
  fontSize: "76px",
  lineHeight: 1,
  fontWeight: 400,
  letterSpacing: "0.04em",
  color: t.color.gold,
  textShadow: "0 2px 14px #f3e2b733",
  selectors: {
    [portraitPhone]: { fontSize: "48px" },
    [landscapePhone]: { fontSize: "38px" },
  },
});
export const bandRule = style({
  width: "min(360px, 60%)",
  height: "1px",
  background: `linear-gradient(90deg, transparent, ${t.color.goldEdge}, transparent)`,
});
export const bandActions = style({
  display: "flex",
  flexWrap: "wrap",
  justifyContent: "center",
  gap: "12px",
  marginTop: "8px",
});
