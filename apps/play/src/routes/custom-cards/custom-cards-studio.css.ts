import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";

const serif = 'Baskerville, "Times New Roman", Georgia, serif';
const sans =
  'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif';

const spin = keyframes({
  from: { transform: "rotate(0deg)" },
  to: { transform: "rotate(360deg)" },
});

export const page = style({
  position: "relative",
  flex: 1,
  minHeight: 0,
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  gap: "12px",
  padding: "14px 20px 14px",
  overflow: "hidden",
  color: t.color.ink,
  fontFamily: sans,
});

export const workspace = style({
  flex: 1,
  minHeight: 0,
  minWidth: 0,
  display: "grid",
  gridTemplateColumns: "22% minmax(0, 1fr) 34%",
  gap: "14px",
  "@media": {
    "(max-width: 1220px)": {
      gridTemplateColumns: "22% minmax(0, 1fr) 33%",
      gap: "10px",
    },
  },
});

const panelBase = {
  background:
    "linear-gradient(180deg, rgba(16, 21, 29, 0.84) 0%, rgba(11, 15, 21, 0.90) 100%)",
  backdropFilter: "blur(16px) saturate(140%)",
  WebkitBackdropFilter: "blur(16px) saturate(140%)",
  border: "1px solid rgba(159, 146, 121, 0.26)",
  borderRadius: "12px",
  boxShadow:
    "inset 0 1px 0 rgba(246, 233, 211, 0.08), 0 12px 32px rgba(0, 0, 0, 0.34)",
  minHeight: 0,
  minWidth: 0,
} as const;

/* Left Vault Panel --------------------------------------------------------- */

export const vaultPanel = style({
  ...panelBase,
  display: "flex",
  flexDirection: "column",
  padding: "14px 12px 12px",
  gap: "10px",
});

export const vaultHeader = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "8px",
});

export const panelTitle = style({
  margin: 0,
  fontFamily: serif,
  fontSize: "18px",
  fontWeight: 500,
  letterSpacing: "0.01em",
  color: "#f6ebd4",
  lineHeight: 1.1,
  whiteSpace: "nowrap",
});

export const vaultHeaderActions = style({
  display: "flex",
  alignItems: "center",
  gap: "6px",
});

export const smallButton = style({
  appearance: "none",
  border: "1px solid rgba(159, 146, 121, 0.34)",
  background: "rgba(25, 30, 39, 0.82)",
  color: "#f2e6cf",
  borderRadius: "7px",
  padding: "5px 9px",
  fontSize: "12px",
  fontWeight: 500,
  lineHeight: 1.2,
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "5px",
  whiteSpace: "nowrap",
  flexShrink: 0,
  transition: "background 120ms ease, border-color 120ms ease, color 120ms ease",
  selectors: {
    "&:hover:not(:disabled)": {
      background: "rgba(42, 47, 56, 0.94)",
      borderColor: "rgba(228, 208, 160, 0.52)",
    },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
    "&:disabled": {
      opacity: 0.48,
      cursor: "not-allowed",
    },
  },
});

export const vaultMetaRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  fontSize: "11px",
  color: "#9f998e",
  letterSpacing: "0.04em",
});

export const archiveFilterButton = style({
  appearance: "none",
  border: "none",
  background: "transparent",
  color: "#b7ad9a",
  fontSize: "11px",
  cursor: "pointer",
  padding: "2px 4px",
  borderRadius: "4px",
  selectors: {
    "&:hover": { color: "#f3e2b7" },
    "&[data-active='true']": { color: "#f3e2b7", textDecoration: "underline" },
  },
});

export const searchWrap = style({
  position: "relative",
  display: "flex",
  alignItems: "center",
});

export const searchIcon = style({
  position: "absolute",
  left: "10px",
  color: "#8f897e",
  pointerEvents: "none",
});

export const searchInput = style({
  width: "100%",
  height: "34px",
  padding: "0 10px 0 32px",
  borderRadius: "8px",
  border: "1px solid rgba(128, 121, 108, 0.34)",
  background: "rgba(9, 13, 19, 0.85)",
  color: "#f4eee4",
  fontSize: "13px",
  outline: "none",
  selectors: {
    "&::placeholder": { color: "#7d7971" },
    "&:focus": {
      borderColor: "rgba(228, 208, 160, 0.65)",
      boxShadow: "0 0 0 2px rgba(228, 208, 160, 0.14)",
    },
  },
});

export const vaultList = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  display: "flex",
  flexDirection: "column",
  gap: "8px",
  paddingRight: "2px",
  scrollbarWidth: "thin",
  scrollbarColor: "#635a48 transparent",
});

export const vaultCard = style({
  position: "relative",
  width: "100%",
  textAlign: "left",
  padding: "10px 11px",
  borderRadius: "9px",
  border: "1px solid rgba(128, 121, 108, 0.25)",
  background: "rgba(18, 22, 30, 0.66)",
  color: "#efe5d3",
  cursor: "pointer",
  display: "flex",
  flexDirection: "column",
  gap: "5px",
  transition: "border-color 130ms ease, background 130ms ease",
  selectors: {
    "&:hover": {
      borderColor: "rgba(212, 188, 138, 0.45)",
      background: "rgba(28, 33, 43, 0.82)",
    },
    "&[data-selected='true']": {
      borderColor: "rgba(219, 191, 132, 0.72)",
      background:
        "linear-gradient(120deg, rgba(164, 130, 72, 0.22) 0%, rgba(24, 24, 26, 0.86) 100%)",
      boxShadow: "inset 0 1px 0 rgba(255, 236, 188, 0.14)",
    },
  },
});

export const vaultCardEyebrow = style({
  fontSize: "10px",
  fontWeight: 600,
  letterSpacing: "0.09em",
  textTransform: "uppercase",
  color: "#d4b57c",
});

export const vaultCardRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "8px",
});

export const vaultCardName = style({
  fontFamily: serif,
  fontSize: "15px",
  fontWeight: 500,
  color: "#f6ebd4",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  minWidth: 0,
});

export const vaultCardSubRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "6px",
});

export const manaCluster = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "3px",
  minHeight: "16px",
});

export const manaImg = style({
  width: "15px",
  height: "15px",
  display: "block",
  flexShrink: 0,
  filter: "drop-shadow(0 1px 1px rgba(0, 0, 0, 0.45))",
});

export const statusDot = style({
  width: "8px",
  height: "8px",
  borderRadius: "999px",
  flexShrink: 0,
  selectors: {
    "&[data-status='ready']": {
      background: "#8eb07c",
      boxShadow: "0 0 6px rgba(142, 176, 124, 0.55)",
    },
    "&[data-status='warning']": {
      background: "#d8a454",
      boxShadow: "0 0 6px rgba(216, 164, 84, 0.55)",
    },
    "&[data-status='error']": {
      background: "#cf6c6c",
      boxShadow: "0 0 6px rgba(207, 108, 108, 0.55)",
    },
  },
});

export const vaultCardMenuRow = style({
  display: "flex",
  alignItems: "center",
  gap: "4px",
  marginTop: "2px",
});

export const microGhostBtn = style({
  appearance: "none",
  border: "1px solid rgba(159, 146, 121, 0.22)",
  background: "rgba(12, 16, 22, 0.65)",
  color: "#bdb4a3",
  borderRadius: "5px",
  padding: "2px 6px",
  fontSize: "10.5px",
  cursor: "pointer",
  selectors: {
    "&:hover": {
      color: "#f6ebd4",
      borderColor: "rgba(228, 208, 160, 0.45)",
    },
    "&[data-danger='true']:hover": {
      color: "#f2a3a3",
      borderColor: "rgba(207, 108, 108, 0.55)",
    },
  },
});

/* Center Authoring Form Panel ---------------------------------------------- */

export const formPanel = style({
  ...panelBase,
  display: "flex",
  flexDirection: "column",
  padding: "14px 16px 12px",
  gap: "10px",
  overflowY: "auto",
  scrollbarWidth: "thin",
  scrollbarColor: "#635a48 transparent",
});

export const formHeaderRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "10px",
});

export const dirtyBadge = style({
  fontSize: "11px",
  color: "#e2c487",
  background: "rgba(216, 164, 84, 0.14)",
  border: "1px solid rgba(216, 164, 84, 0.35)",
  borderRadius: "999px",
  padding: "2px 9px",
});

export const rowGrid2 = style({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1.12fr) minmax(0, 1fr)",
  gap: "12px",
  alignItems: "end",
});

export const rowGridType = style({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto",
  gap: "12px",
  alignItems: "end",
});

export const rowGrid3 = style({
  display: "grid",
  gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
  gap: "10px",
  alignItems: "end",
});

export const rowGrid4 = style({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto minmax(0, 1fr) minmax(0, 1fr)",
  gap: "10px",
  alignItems: "end",
});

export const fieldGroup = style({
  display: "flex",
  flexDirection: "column",
  gap: "5px",
  minWidth: 0,
});

export const fieldLabelRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "8px",
});

export const fieldLabel = style({
  fontSize: "12px",
  fontWeight: 500,
  color: "#c8bfb0",
  letterSpacing: "0.02em",
});

export const inputBase = style({
  width: "100%",
  height: "36px",
  padding: "0 11px",
  borderRadius: "8px",
  border: "1px solid rgba(130, 122, 108, 0.36)",
  background: "rgba(9, 13, 19, 0.88)",
  color: "#f5efe3",
  fontFamily: serif,
  fontSize: "15px",
  outline: "none",
  transition: "border-color 120ms ease, box-shadow 120ms ease",
  selectors: {
    "&::placeholder": { color: "#6c6963" },
    "&:focus": {
      borderColor: "rgba(228, 208, 160, 0.72)",
      boxShadow: "0 0 0 2px rgba(228, 208, 160, 0.14)",
    },
  },
});

export const selectBase = style({
  width: "100%",
  height: "36px",
  padding: "0 28px 0 10px",
  borderRadius: "8px",
  border: "1px solid rgba(130, 122, 108, 0.36)",
  background: "rgba(13, 18, 26, 0.92)",
  color: "#f4eee4",
  fontSize: "13px",
  outline: "none",
  cursor: "pointer",
  selectors: {
    "&:focus": {
      borderColor: "rgba(228, 208, 160, 0.72)",
      boxShadow: "0 0 0 2px rgba(228, 208, 160, 0.14)",
    },
  },
});

export const manaFieldRow = style({
  display: "flex",
  alignItems: "center",
  gap: "6px",
  minHeight: "36px",
});

export const manaInput = style({
  flex: 1,
  minWidth: "74px",
  height: "36px",
  padding: "0 9px",
  borderRadius: "8px",
  border: "1px solid rgba(130, 122, 108, 0.36)",
  background: "rgba(9, 13, 19, 0.88)",
  color: "#f5efe3",
  fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
  fontSize: "13px",
  outline: "none",
  selectors: {
    "&:focus": {
      borderColor: "rgba(228, 208, 160, 0.72)",
      boxShadow: "0 0 0 2px rgba(228, 208, 160, 0.14)",
    },
  },
});

export const manaPalette = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "3px",
  flexShrink: 0,
});

export const manaSymbolBtn = style({
  appearance: "none",
  width: "22px",
  height: "22px",
  padding: 0,
  borderRadius: "999px",
  border: "1px solid rgba(159, 146, 121, 0.28)",
  background: "rgba(18, 22, 30, 0.9)",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  cursor: "pointer",
  transition: "transform 90ms ease, border-color 110ms ease",
  selectors: {
    "&:hover": {
      transform: "translateY(-1px)",
      borderColor: "rgba(243, 226, 183, 0.75)",
    },
  },
});

export const ptRow = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "5px",
});

export const statInput = style({
  width: "46px",
  height: "36px",
  textAlign: "center",
  borderRadius: "8px",
  border: "1px solid rgba(130, 122, 108, 0.36)",
  background: "rgba(9, 13, 19, 0.88)",
  color: "#f5efe3",
  fontFamily: serif,
  fontSize: "15px",
  outline: "none",
  selectors: {
    "&:focus": {
      borderColor: "rgba(228, 208, 160, 0.72)",
    },
  },
});

export const statSlash = style({
  color: "#8f897e",
  fontSize: "15px",
  fontWeight: 500,
});

export const segmented = style({
  display: "inline-flex",
  padding: "2px",
  borderRadius: "8px",
  border: "1px solid rgba(130, 122, 108, 0.34)",
  background: "rgba(9, 13, 19, 0.88)",
  height: "36px",
  alignItems: "center",
});

export const segmentBtn = style({
  appearance: "none",
  border: "none",
  background: "transparent",
  color: "#a79f91",
  fontSize: "12px",
  fontWeight: 500,
  padding: "0 10px",
  height: "30px",
  borderRadius: "6px",
  cursor: "pointer",
  selectors: {
    "&[data-active='true']": {
      background: "rgba(180, 148, 92, 0.28)",
      color: "#f7e8c4",
      boxShadow: "inset 0 1px 0 rgba(255, 238, 195, 0.22)",
    },
  },
});

export const colorIndicatorRow = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "4px",
});

export const colorPipBtn = style({
  appearance: "none",
  width: "21px",
  height: "21px",
  borderRadius: "5px",
  border: "1px solid rgba(130, 122, 108, 0.34)",
  background: "rgba(16, 20, 28, 0.82)",
  color: "#9a9387",
  fontSize: "10.5px",
  fontWeight: 700,
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  selectors: {
    "&[data-active='true']": {
      borderColor: "rgba(243, 226, 183, 0.85)",
      color: "#15120d",
      boxShadow: "0 0 8px rgba(243, 226, 183, 0.25)",
    },
  },
});

export const rulesTextarea = style({
  width: "100%",
  height: "228px",
  minHeight: "152px",
  maxHeight: "320px",
  resize: "vertical",
  padding: "11px 12px",
  borderRadius: "9px",
  border: "1px solid rgba(130, 122, 108, 0.36)",
  background: "rgba(9, 13, 19, 0.90)",
  color: "#f5efe3",
  fontFamily: serif,
  fontSize: "15.5px",
  lineHeight: 1.48,
  outline: "none",
  selectors: {
    "&::placeholder": { color: "#68645d" },
    "&:focus": {
      borderColor: "rgba(228, 208, 160, 0.72)",
      boxShadow: "0 0 0 2px rgba(228, 208, 160, 0.14)",
    },
  },
});

export const diagnosticsRow = style({
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "8px",
});

export const diagnosticChips = style({
  display: "flex",
  flexDirection: "column",
  gap: "7px",
  flex: 1,
  minWidth: 0,
});

export const diagChip = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "8px",
  padding: "7px 12px",
  borderRadius: "8px",
  fontSize: "13px",
  fontWeight: 500,
  lineHeight: 1.35,
  maxWidth: "100%",
  selectors: {
    "&[data-severity='warning']": {
      background: "rgba(181, 132, 46, 0.24)",
      border: "1px solid rgba(224, 174, 88, 0.56)",
      color: "#f6dba4",
    },
    "&[data-severity='error']": {
      background: "rgba(168, 48, 56, 0.30)",
      border: "1px solid rgba(226, 110, 114, 0.64)",
      color: "#ffd0d0",
    },
    "&[data-severity='info']": {
      background: "rgba(86, 134, 178, 0.20)",
      border: "1px solid rgba(124, 171, 214, 0.48)",
      color: "#d4e8fa",
    },
  },
});

export const standardizeGroup = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "6px",
  flexShrink: 0,
});

export const suggestionPill = style({
  fontSize: "11px",
  color: "#e8cb92",
  background: "rgba(181, 132, 46, 0.18)",
  border: "1px solid rgba(216, 164, 84, 0.36)",
  borderRadius: "6px",
  padding: "3px 8px",
});

export const bottomFormGrid = style({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1.35fr) 156px",
  gap: "12px",
  alignItems: "stretch",
});

export const flavorTextarea = style({
  width: "100%",
  minHeight: "66px",
  resize: "none",
  padding: "8px 10px",
  borderRadius: "8px",
  border: "1px solid rgba(130, 122, 108, 0.30)",
  background: "rgba(9, 13, 19, 0.84)",
  color: "#dfd5c3",
  fontFamily: serif,
  fontStyle: "italic",
  fontSize: "13.5px",
  lineHeight: 1.38,
  outline: "none",
  selectors: {
    "&::placeholder": { color: "#68645d" },
    "&:focus": {
      borderColor: "rgba(228, 208, 160, 0.65)",
    },
  },
});

export const artworkDropTile = style({
  width: "100%",
  minHeight: "66px",
  borderRadius: "8px",
  border: "1px dashed rgba(159, 146, 121, 0.42)",
  background: "rgba(12, 16, 23, 0.86)",
  color: "#c9c0b0",
  cursor: "pointer",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  gap: "4px",
  padding: "6px 8px",
  fontSize: "11.5px",
  transition: "border-color 120ms ease, background 120ms ease",
  selectors: {
    "&:hover": {
      borderColor: "rgba(228, 208, 160, 0.72)",
      background: "rgba(24, 30, 40, 0.92)",
    },
  },
});

export const artworkThumb = style({
  width: "54px",
  height: "34px",
  borderRadius: "5px",
  objectFit: "cover",
  border: "1px solid rgba(228, 208, 160, 0.35)",
});

export const disclosureBox = style({
  borderTop: "1px solid rgba(159, 146, 121, 0.18)",
  paddingTop: "8px",
  display: "flex",
  flexDirection: "column",
  gap: "8px",
});

export const disclosureTrigger = style({
  appearance: "none",
  border: "none",
  background: "transparent",
  color: "#c8bfb0",
  fontSize: "12.5px",
  fontWeight: 500,
  padding: "2px 0",
  cursor: "pointer",
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  width: "100%",
  selectors: {
    "&:hover": { color: "#f6ebd4" },
  },
});

/* Right Live Card Stage Panel ---------------------------------------------- */

export const stagePanel = style({
  ...panelBase,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "16px 16px 14px",
  gap: "12px",
  background:
    "radial-gradient(ellipse at 50% 42%, rgba(44, 56, 76, 0.34) 0%, rgba(12, 16, 23, 0.92) 72%)",
});

export const cardViewport = style({
  flex: 1,
  minHeight: 0,
  width: "100%",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  position: "relative",
});

export const cardHolder = style({
  width: "92%",
  maxWidth: "496px",
  maxHeight: "100%",
  aspectRatio: "1062 / 1482",
  borderRadius: "16px",
  overflow: "hidden",
  boxShadow:
    "0 22px 48px rgba(0, 0, 0, 0.64), 0 0 0 1px rgba(243, 226, 183, 0.18)",
  transition: "transform 160ms ease",
});

export const framingBar = style({
  width: "100%",
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "6px",
  paddingTop: "8px",
  borderTop: "1px solid rgba(159, 146, 121, 0.18)",
  flexWrap: "nowrap",
});

export const zoomControl = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "6px",
  fontSize: "11.5px",
  color: "#b9b0a1",
  flexShrink: 0,
  justifyContent: "flex-end",
});

export const zoomSlider = style({
  width: "74px",
  accentColor: "#d6b67c",
  cursor: "pointer",
});

/* Bottom Action Dock ------------------------------------------------------- */

export const dockWrap = style({
  position: "relative",
  flexShrink: 0,
});

export const dock = style({
  ...panelBase,
  padding: "9px 14px",
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "12px",
  minHeight: "54px",
});

export const dockLeft = style({
  display: "flex",
  alignItems: "center",
  gap: "8px",
  minWidth: 0,
  flexWrap: "wrap",
});

export const statusPill = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "7px",
  padding: "5px 11px",
  borderRadius: "8px",
  fontSize: "12.5px",
  fontWeight: 600,
  whiteSpace: "nowrap",
  selectors: {
    "&[data-status='ready']": {
      background: "rgba(104, 134, 92, 0.28)",
      border: "1px solid rgba(148, 184, 132, 0.45)",
      color: "#d4e8c9",
    },
    "&[data-status='warning']": {
      background: "rgba(176, 128, 46, 0.28)",
      border: "1px solid rgba(224, 174, 88, 0.48)",
      color: "#f3d8a3",
    },
    "&[data-status='error']": {
      background: "rgba(156, 52, 58, 0.28)",
      border: "1px solid rgba(207, 108, 108, 0.50)",
      color: "#f5b8b8",
    },
    "&[data-status='checking']": {
      background: "rgba(74, 90, 114, 0.26)",
      border: "1px solid rgba(132, 152, 182, 0.36)",
      color: "#ced9e8",
    },
  },
});

export const astChip = style({
  display: "inline-flex",
  alignItems: "center",
  padding: "4px 9px",
  borderRadius: "7px",
  background: "rgba(30, 36, 46, 0.85)",
  border: "1px solid rgba(159, 146, 121, 0.22)",
  color: "#ddd4c4",
  fontSize: "12px",
  whiteSpace: "nowrap",
});

export const inspectBtn = style({
  appearance: "none",
  border: "none",
  background: "transparent",
  color: "#d4b57c",
  fontSize: "12.5px",
  fontWeight: 500,
  padding: "4px 8px",
  borderRadius: "6px",
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  gap: "4px",
  selectors: {
    "&:hover": {
      background: "rgba(212, 181, 124, 0.12)",
      color: "#f5e3ba",
    },
  },
});

export const dockRight = style({
  display: "flex",
  alignItems: "center",
  gap: "8px",
  flexShrink: 0,
});

export const dockDivider = style({
  width: "1px",
  height: "24px",
  background: "rgba(159, 146, 121, 0.26)",
  marginInline: "2px",
});

export const dockActionBtn = style({
  appearance: "none",
  height: "36px",
  padding: "0 13px",
  borderRadius: "8px",
  border: "1px solid rgba(159, 146, 121, 0.36)",
  background: "rgba(22, 27, 36, 0.88)",
  color: "#f4eee4",
  fontSize: "13px",
  fontWeight: 500,
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "6px",
  transition: "background 120ms ease, border-color 120ms ease",
  selectors: {
    "&:hover:not(:disabled)": {
      background: "rgba(38, 44, 55, 0.96)",
      borderColor: "rgba(228, 208, 160, 0.62)",
    },
    "&:disabled": {
      opacity: 0.45,
      cursor: "not-allowed",
    },
  },
});

export const goldCtaBtn = style({
  appearance: "none",
  height: "36px",
  padding: "0 16px",
  borderRadius: "8px",
  border: "1px solid #e7cd96",
  background: "linear-gradient(180deg, #cfb076 0%, #a5834a 100%)",
  color: "#15110a",
  fontSize: "13.5px",
  fontWeight: 600,
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "7px",
  boxShadow: "0 4px 16px rgba(196, 159, 92, 0.28)",
  transition: "filter 120ms ease, transform 100ms ease",
  selectors: {
    "&:hover:not(:disabled)": {
      filter: "brightness(1.08)",
    },
    "&:active:not(:disabled)": {
      transform: "translateY(1px)",
    },
    "&:disabled": {
      opacity: 1,
      background: "rgba(116, 98, 66, 0.34)",
      borderColor: "rgba(186, 158, 104, 0.38)",
      color: "#cfc3aa",
      boxShadow: "none",
      cursor: "not-allowed",
    },
  },
});

/* AST Inspect Anchored Drawer ---------------------------------------------- */

export const astDrawer = style({
  position: "absolute",
  left: 0,
  right: 0,
  bottom: "calc(100% + 10px)",
  zIndex: 25,
  background: "rgba(13, 17, 24, 0.98)",
  backdropFilter: "blur(18px)",
  border: "1px solid rgba(224, 194, 134, 0.46)",
  borderRadius: "12px",
  boxShadow: "0 -18px 48px rgba(0, 0, 0, 0.78), 0 0 0 1px rgba(243, 226, 183, 0.12)",
  padding: "16px 20px",
  display: "grid",
  gridTemplateColumns: "minmax(0, 1.25fr) minmax(0, 1fr)",
  gap: "18px",
  maxHeight: "310px",
  overflowY: "auto",
});

export const astSectionTitle = style({
  margin: "0 0 8px",
  fontSize: "11.5px",
  fontWeight: 600,
  textTransform: "uppercase",
  letterSpacing: "0.08em",
  color: "#c8b288",
});

export const astList = style({
  display: "flex",
  flexDirection: "column",
  gap: "6px",
});

export const astRow = style({
  display: "flex",
  alignItems: "flex-start",
  gap: "8px",
  padding: "6px 9px",
  borderRadius: "7px",
  background: "rgba(22, 27, 36, 0.78)",
  border: "1px solid rgba(159, 146, 121, 0.16)",
  fontSize: "12.5px",
  color: "#e9e0cf",
});

export const astKindBadge = style({
  fontSize: "10.5px",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
  color: "#d9bd85",
  background: "rgba(181, 132, 46, 0.16)",
  padding: "2px 6px",
  borderRadius: "5px",
  flexShrink: 0,
});

/* Patron Quick Test Popover ------------------------------------------------ */

export const playPopover = style({
  position: "absolute",
  right: 0,
  bottom: "calc(100% + 12px)",
  zIndex: 30,
  width: "356px",
  background: "rgba(14, 18, 26, 0.99)",
  border: "1px solid rgba(228, 208, 160, 0.50)",
  borderRadius: "12px",
  boxShadow: "0 20px 56px rgba(0, 0, 0, 0.84), 0 0 0 1px rgba(243, 226, 183, 0.14)",
  padding: "16px",
  display: "flex",
  flexDirection: "column",
  gap: "12px",
});

/* Modals (S1-06 Import & S1-09 Patreon) ------------------------------------ */

export const modalBackdrop = style({
  position: "fixed",
  inset: 0,
  zIndex: 60,
  background: "rgba(5, 8, 13, 0.66)",
  backdropFilter: "blur(2px)",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  padding: "24px",
});

export const modalCard = style({
  width: "100%",
  maxWidth: "630px",
  background:
    "linear-gradient(180deg, rgba(20, 25, 34, 0.99) 0%, rgba(13, 17, 24, 0.99) 100%)",
  border: "1px solid rgba(212, 181, 124, 0.42)",
  borderRadius: "16px",
  boxShadow: "0 30px 80px rgba(0, 0, 0, 0.80)",
  padding: "26px 28px 24px",
  display: "flex",
  flexDirection: "column",
  gap: "18px",
  color: "#f4eee4",
});

export const patreonModalCard = style({
  width: "100%",
  maxWidth: "780px",
  background:
    "linear-gradient(180deg, rgba(20, 25, 34, 0.99) 0%, rgba(13, 17, 24, 0.99) 100%)",
  border: "1px solid rgba(212, 181, 124, 0.46)",
  borderRadius: "16px",
  boxShadow: "0 30px 80px rgba(0, 0, 0, 0.82)",
  padding: "30px 32px",
  display: "grid",
  gridTemplateColumns: "268px minmax(0, 1fr)",
  gap: "30px",
  alignItems: "center",
  color: "#f4eee4",
  position: "relative",
});

export const modalHeader = style({
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "space-between",
  gap: "12px",
});

export const modalTitle = style({
  margin: 0,
  fontFamily: serif,
  fontSize: "26px",
  fontWeight: 500,
  color: "#f8eed9",
  lineHeight: 1.15,
});

export const modalSubtitle = style({
  margin: "8px 0 0",
  fontSize: "14.5px",
  color: "#c2b9a8",
  lineHeight: 1.48,
});

export const iconCloseBtn = style({
  appearance: "none",
  border: "none",
  background: "transparent",
  color: "#aba294",
  width: "30px",
  height: "30px",
  borderRadius: "7px",
  cursor: "pointer",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  selectors: {
    "&:hover": {
      background: "rgba(255, 255, 255, 0.08)",
      color: "#f8eed9",
    },
  },
});

export const dropzone = style({
  border: "1.5px dashed rgba(176, 157, 120, 0.48)",
  borderRadius: "12px",
  background: "rgba(10, 14, 21, 0.78)",
  padding: "28px 24px",
  minHeight: "236px",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  gap: "14px",
  textAlign: "center",
  cursor: "pointer",
  transition: "border-color 140ms ease, background 140ms ease",
  selectors: {
    "&:hover, &[data-drag='true']": {
      borderColor: "rgba(230, 200, 138, 0.78)",
      background: "rgba(26, 33, 45, 0.72)",
    },
  },
});

export const visionPreviewRow = style({
  width: "100%",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "14px",
  textAlign: "center",
});

export const progressTrack = style({
  width: "100%",
  maxWidth: "420px",
  height: "9px",
  borderRadius: "999px",
  background: "rgba(255, 255, 255, 0.10)",
  overflow: "hidden",
});

export const progressFill = style({
  height: "100%",
  borderRadius: "999px",
  background: "linear-gradient(90deg, #c6a05c 0%, #f0d79c 100%)",
  transition: "width 220ms ease",
});

export const modeBadgesRow = style({
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  gap: "10px",
});

export const modeBadge = style({
  padding: "10px 12px",
  borderRadius: "9px",
  border: "1px solid rgba(145, 136, 118, 0.30)",
  background: "rgba(15, 19, 27, 0.88)",
  color: "#cfc5b4",
  fontSize: "13px",
  textAlign: "center",
  selectors: {
    "&[data-active='true']": {
      borderColor: "rgba(224, 190, 122, 0.78)",
      background: "rgba(176, 134, 62, 0.18)",
      color: "#f8e8c6",
    },
  },
});

export const modalFooter = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "flex-end",
  gap: "10px",
});

export const benefitList = style({
  listStyle: "none",
  margin: "18px 0 22px",
  padding: 0,
  display: "flex",
  flexDirection: "column",
  gap: "13px",
});

export const benefitItem = style({
  display: "flex",
  alignItems: "center",
  gap: "11px",
  fontSize: "14.5px",
  color: "#f0e6d5",
  lineHeight: 1.4,
});

export const benefitIcon = style({
  color: "#d9b97e",
  flexShrink: 0,
});

export const toastBanner = style({
  position: "fixed",
  top: "88px",
  right: "26px",
  zIndex: 70,
  background: "rgba(17, 23, 32, 0.96)",
  border: "1px solid rgba(212, 181, 124, 0.48)",
  borderRadius: "10px",
  padding: "10px 14px",
  color: "#f6ebd4",
  fontSize: "13px",
  boxShadow: "0 12px 32px rgba(0, 0, 0, 0.55)",
});

export const spinnerIcon = style({
  animation: `${spin} 1s linear infinite`,
});

globalStyle(`${cardHolder} svg`, {
  display: "block",
  width: "100%",
  height: "100%",
});
