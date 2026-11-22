import { style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";

export const page = style({
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  gap: 20,
  padding: "24px 28px 36px",
  overflowY: "auto",
  color: t.color.ink,
  "@media": {
    "(max-width: 720px)": {
      padding: "16px 16px 104px",
    },
  },
});

export const header = style({
  display: "flex",
  flexWrap: "wrap",
  alignItems: "baseline",
  justifyContent: "space-between",
  gap: 12,
});

export const title = style({
  margin: 0,
  fontSize: 32,
  fontWeight: 500,
  color: t.color.gold,
});

export const subtitle = style({
  margin: 0,
  fontSize: t.font.hint,
  color: t.color.muted,
});

export const grid = style({
  display: "grid",
  gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))",
  gap: 18,
  alignItems: "start",
});

export const panel = style({
  padding: "18px 20px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.edge}`,
  background: t.color.surface,
  backdropFilter: t.effect.backdrop,
  WebkitBackdropFilter: t.effect.backdrop,
  display: "flex",
  flexDirection: "column",
  gap: 14,
});

export const panelTitle = style({
  margin: 0,
  fontSize: t.font.small,
  color: t.color.gold,
});

export const rankCard = style({
  display: "flex",
  flexDirection: "column",
  gap: 10,
  padding: "12px 14px",
  borderRadius: 8,
  border: `1px solid color-mix(in srgb, ${t.color.edge} 60%, transparent)`,
  background: t.color.field,
});

export const rankHead = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
});

export const formatName = style({
  fontSize: t.font.hint,
  color: t.color.ink,
  fontWeight: 600,
});

export const recordText = style({
  fontSize: 16,
  color: t.color.muted,
});

export const questRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
  padding: "10px 12px",
  borderRadius: 8,
  border: `1px solid color-mix(in srgb, ${t.color.edge} 60%, transparent)`,
  background: t.color.field,
});

export const questMeta = style({
  display: "flex",
  flexDirection: "column",
  gap: 4,
  minWidth: 0,
});

export const controlsRow = style({
  display: "flex",
  flexWrap: "wrap",
  gap: 12,
  alignItems: "center",
});

export const tableWrap = style({
  width: "100%",
  overflowX: "auto",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.edge}`,
  background: t.color.surface,
});

export const table = style({
  width: "100%",
  borderCollapse: "collapse",
  textAlign: "left",
  fontSize: t.font.hint,
});

export const th = style({
  padding: "12px 14px",
  color: t.color.muted,
  borderBottom: `1px solid ${t.color.edge}`,
  fontWeight: 500,
  whiteSpace: "nowrap",
  "@media": {
    "(max-width: 600px)": {
      padding: "10px 8px",
    },
  },
});

export const td = style({
  padding: "12px 14px",
  borderBottom: `1px solid color-mix(in srgb, ${t.color.edge} 40%, transparent)`,
  verticalAlign: "middle",
  "@media": {
    "(max-width: 600px)": {
      padding: "10px 8px",
    },
  },
});

export const hideMobile = style({
  "@media": {
    "(max-width: 600px)": {
      display: "none",
    },
  },
});

export const playerCell = style({
  maxWidth: 260,
  "@media": {
    "(max-width: 600px)": {
      maxWidth: 120,
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap",
    },
  },
});

export const playerButton = style({
  background: "none",
  border: "none",
  padding: 0,
  color: t.color.ink,
  font: "inherit",
  cursor: "pointer",
  textDecoration: "underline",
  textUnderlineOffset: 3,
  selectors: {
    "&:hover": {
      color: t.color.gold,
    },
  },
});

export const center = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  padding: 24,
});
