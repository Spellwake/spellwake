import { style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";

export const tableSurface = style({
  position: "relative",
  width: "100%",
  height: "100%",
  display: "flex",
  flexDirection: "column",
  justifyContent: "space-between",
  paddingTop: 84,
  paddingBottom: 210,
  paddingLeft: 24,
  paddingRight: 368,
  boxSizing: "border-box",
  gap: 12,
  overflow: "hidden",
  "@media": {
    "(max-width: 960px)": {
      paddingTop: 136,
      paddingRight: 20,
      paddingLeft: 20,
      paddingBottom: 376,
    },
  },
});

export const boardHalf = style({
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  justifyContent: "center",
  gap: 8,
  padding: "10px 14px",
  borderRadius: t.size.radius,
  background: `color-mix(in srgb, ${t.color.surface} 62%, transparent)`,
  border: `1px solid color-mix(in srgb, ${t.color.edge} 48%, transparent)`,
  overflow: "auto",
});

export const boardHeader = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
  fontSize: t.font.hint,
  color: t.color.muted,
});

export const boardRow = style({
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: 10,
  minHeight: 72,
});

export const cardTile = style({
  position: "relative",
  width: 98,
  height: 136,
  borderRadius: 8,
  border: `1px solid ${t.color.edge}`,
  background: t.color.field,
  color: t.color.ink,
  padding: 6,
  display: "flex",
  flexDirection: "column",
  justifyContent: "space-between",
  textAlign: "left",
  cursor: "pointer",
  overflow: "hidden",
  flexShrink: 0,
  "@media": {
    "(max-width: 960px)": {
      width: 76,
      height: 106,
    },
  },
  transition:
    "transform 160ms ease, border-color 160ms ease, box-shadow 160ms ease",
  selectors: {
    "&:hover": {
      borderColor: t.color.goldEdge,
      transform: "translateY(-3px)",
    },
    '&[data-tapped="true"]': {
      transform: "rotate(14deg)",
      opacity: 0.85,
    },
    '&[data-selected="true"]': {
      borderColor: t.color.focus,
      boxShadow: t.ring.glow,
    },
    '&[data-playable="true"]': {
      borderColor: t.color.goldEdge,
      boxShadow: `0 0 0 1px ${t.color.goldEdge}`,
    },
  },
});

export const cardTileArt = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  objectFit: "cover",
  pointerEvents: "none",
});

export const cardTileScrim = style({
  position: "relative",
  zIndex: 1,
  display: "flex",
  flexDirection: "column",
  justifyContent: "space-between",
  height: "100%",
  pointerEvents: "none",
});

export const cardTitle = style({
  fontSize: 13,
  fontWeight: 600,
  lineHeight: 1.15,
  color: t.color.ink,
  textShadow: "0 1px 4px rgba(0, 0, 0, 0.9)",
  background: "rgba(10, 14, 20, 0.72)",
  padding: "2px 4px",
  borderRadius: 4,
});

export const cardMeta = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 4,
  fontSize: 12,
  color: t.color.gold,
  background: "rgba(10, 14, 20, 0.82)",
  padding: "2px 5px",
  borderRadius: 4,
});

export const handTray = style({
  display: "flex",
  alignItems: "flex-end",
  justifyContent: "center",
  gap: 10,
  width: "100%",
  height: "100%",
  padding: "6px 16px 10px",
  boxSizing: "border-box",
  overflowX: "auto",
});

export const zoneCluster = style({
  display: "flex",
  alignItems: "center",
  gap: 8,
  flexShrink: 0,
});

export const zonePill = style({
  padding: "6px 10px",
  borderRadius: 8,
  border: `1px solid ${t.color.edge}`,
  background: t.color.surface,
  color: t.color.ink,
  fontSize: 14,
  cursor: "pointer",
  whiteSpace: "nowrap",
  selectors: {
    "&:hover": {
      borderColor: t.color.goldEdge,
    },
  },
});

export const centerStrip = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
  flexWrap: "wrap",
});

export const decisionCard = style({
  flex: 1,
  minWidth: 260,
  maxWidth: 680,
  padding: "12px 16px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.goldEdge}`,
  background: `color-mix(in srgb, ${t.color.menu} 94%, transparent)`,
  display: "flex",
  flexDirection: "column",
  gap: 10,
});

export const decisionTitle = style({
  margin: 0,
  fontSize: t.font.small,
  color: t.color.gold,
});

export const decisionRow = style({
  display: "flex",
  flexWrap: "wrap",
  gap: 8,
  alignItems: "center",
});

export const manaBar = style({
  display: "flex",
  alignItems: "center",
  gap: 6,
  flexWrap: "wrap",
  fontSize: 15,
  color: t.color.gold,
  padding: "4px 10px",
  borderRadius: 999,
  background: t.color.surface,
  border: `1px solid ${t.color.edge}`,
});

export const stackChip = style({
  padding: "6px 10px",
  borderRadius: 8,
  border: `1px solid ${t.color.goldEdge}`,
  background: t.color.emphasis,
  color: t.color.ink,
  fontSize: 14,
});

export const chatDrawer = style({
  display: "flex",
  flexDirection: "column",
  gap: 12,
  minHeight: 320,
  maxHeight: "65vh",
});

export const chatList = style({
  flex: 1,
  overflowY: "auto",
  display: "flex",
  flexDirection: "column",
  gap: 8,
  paddingRight: 4,
});

export const inspectorBody = style({
  display: "grid",
  gridTemplateColumns: "minmax(180px, 240px) 1fr",
  gap: 16,
  alignItems: "start",
});

export const inspectorImg = style({
  width: "100%",
  borderRadius: 10,
  border: `1px solid ${t.color.edge}`,
});

export const inspectorText = style({
  display: "flex",
  flexDirection: "column",
  gap: 10,
  color: t.color.ink,
  fontSize: t.font.hint,
});

export const emptyShell = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  padding: 24,
});

export const statusBadge = style({
  alignSelf: "center",
  padding: "5px 12px",
  borderRadius: 999,
  border: `1px solid ${t.color.edge}`,
  background: t.color.surface,
  color: t.color.muted,
  fontSize: 13,
  lineHeight: 1.2,
  whiteSpace: "nowrap",
});
