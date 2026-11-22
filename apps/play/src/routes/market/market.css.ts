import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * Market Listings, as approved (ML-01 to ML-11). The page frame, filter panel
 * and inspector column are shared with Collection Cards; these are the
 * toolbar's two rows, the listing rows, the inspector's price and action and
 * the dialogs' parts. Lengths are in the approved references' pixel scale.
 */
const TOOLBAR_STACKED = "cards (inline-size < 52rem)";
const phone = ':root[data-device="phone"] &';
const rule = "#a698774c";
const danger = "#e98a7a";

export const toolbar = style({
  position: "relative",
  zIndex: 2,
  flexShrink: 0,
  display: "grid",
  gridTemplateColumns: "minmax(0,1fr) max-content max-content max-content",
  alignItems: "center",
  gap: "22px",
  "@container": {
    [TOOLBAR_STACKED]: {
      gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
      gap: "14px",
    },
  },
});
globalStyle(`${toolbar} > *`, { minWidth: 0 });
export const search = style({
  minWidth: 0,
  "@container": { [TOOLBAR_STACKED]: { gridColumn: "1 / -1" } },
});
export const create = style({
  whiteSpace: "nowrap",
  "@container": { [TOOLBAR_STACKED]: { gridColumn: "1 / -1" } },
});
export const secondRow = style({
  flexShrink: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  flexWrap: "wrap",
  gap: "12px 20px",
  padding: "16px 4px 14px",
  "@container": {
    [TOOLBAR_STACKED]: { flexWrap: "nowrap", gap: "12px", paddingTop: "12px" },
  },
  borderBottom: `1px solid ${rule}`,
});
globalStyle(`${secondRow} > *`, { flex: "none", width: "auto" });
globalStyle(`${secondRow} > :last-child`, {
  width: "min(236px, 100%)",
  "@container": {
    [TOOLBAR_STACKED]: {
      width: "auto",
      flex: "1 1 0",
      minWidth: 0,
      maxWidth: "236px",
    },
  },
});

export const rows = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  padding: "20px 0 40px",
  selectors: { [phone]: { paddingBottom: "150px" } },
});
export const srOnly = style({
  position: "absolute",
  width: "1px",
  height: "1px",
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
});

/** The inspector: the summary, a rule, the total and the one action. */
export const inspectorBody = style({
  display: "flex",
  flexDirection: "column",
  gap: "0",
  minHeight: 0,
  overflowY: "auto",
});
export const inspectorPrice = style({
  margin: "22px 0 24px",
  paddingTop: "22px",
  borderTop: `1px solid ${rule}`,
  fontSize: "44px",
  color: shellTokens.ink,
});
export const inspectorAction = style({
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
  selectors: {
    "&&": { width: "100%", height: "56px", fontSize: "27px" },
    '&&[data-tone="danger"]': { color: danger, borderColor: "#b7584a" },
  },
});

/** A pack's drawn wrapper inside a listing's artwork box. */
export const packArt = style({
  display: "block",
  width: "100%",
  maxWidth: "100%",
});

export const dialogItem = style({ padding: "8px 0 4px" });
export const total = style({
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  gap: "16px",
  padding: "12px 4px",
  borderBlock: `1px solid ${rule}`,
  font: `32px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const dialogText = style({
  margin: "18px 0 0",
  font: `24px/1.35 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
/** Why a held-open command cannot go on, or that its outcome is unknown. */
export const stageLine = style({
  margin: "14px 0 0",
  textAlign: "center",
  font: `23px/1.3 ${shellTokens.serif}`,
  selectors: {
    '&[data-tone="unknown"]': { color: "#e2c27f" },
    '&[data-tone="known"]': { color: shellTokens.ink },
  },
});

/** Create listing keeps one fixed shell for both stages. */
export const createDialog = style({
  // A phone's drawer fits its content instead; a fixed height would hide the terms.
  selectors: {
    [`:root:not([data-device="phone"]) &&`]: { height: "min(706px, 100dvh)" },
  },
});
export const pickerBody = style({
  display: "flex",
  flexDirection: "column",
  gap: "16px",
  minHeight: 0,
  paddingTop: "16px",
});
export const pickerList = style({
  minHeight: "260px",
  maxHeight: "430px",
  overflowY: "auto",
  overscrollBehavior: "contain",
  border: `1px solid ${rule}`,
  borderRadius: "12px",
  padding: "6px",
  selectors: { [phone]: { maxHeight: "none" } },
});
export const pickerQuiet = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "14px",
  minHeight: "240px",
  font: `24px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  width: "26px",
  height: "26px",
  borderRadius: "50%",
  border: "3px solid #ffffff26",
  borderTopColor: "#e9dcc4",
  "@media": { [standardMotion]: { animation: `${spin} 1s linear infinite` } },
});

export const terms = style({
  display: "flex",
  flexDirection: "column",
  gap: "16px",
  paddingTop: "6px",
});
export const backLink = style({
  appearance: "none",
  alignSelf: "flex-start",
  display: "inline-flex",
  alignItems: "center",
  gap: "6px",
  minHeight: "40px",
  padding: "0 6px 0 0",
  border: 0,
  background: "transparent",
  color: shellTokens.ink,
  font: `24px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&:disabled": { opacity: 0.5, cursor: "default" },
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "2px",
    },
  },
  "@media": { "(pointer: coarse)": { minHeight: "44PX" } },
});
globalStyle(`${backLink} svg`, { width: "22px", height: "22px" });
export const termsItem = style({
  display: "grid",
  gridTemplateColumns: "170px minmax(0,1fr)",
  gap: "28px",
  alignItems: "start",
  selectors: {
    [phone]: { gridTemplateColumns: "110px minmax(0,1fr)", gap: "18px" },
  },
});
export const termsArt = style({ display: "block" });
export const termsCopy = style({
  display: "grid",
  gap: "8px",
  paddingTop: "10px",
  font: `25px/1.25 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
globalStyle(`${termsCopy} p`, { margin: 0 });
export const termsName = style({
  margin: 0,
  font: `34px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
});
export const field = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,190px) minmax(0,1fr)",
  alignItems: "center",
  gap: "12px 20px",
  padding: "14px 20px",
  border: `1px solid ${rule}`,
  borderRadius: "12px",
  selectors: { [phone]: { gridTemplateColumns: "minmax(0,1fr)" } },
});
export const fieldLabel = style({
  font: `25px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const fieldControl = style({ display: "grid", gap: "6px", minWidth: 0 });
/** The stepper keeps its own width rather than stretching across the field. */
globalStyle(`${fieldControl} > [role="group"]`, {
  justifySelf: "start",
  width: "min(100%, 280px)",
});
export const fieldError = style({
  margin: 0,
  font: `21px/1.25 ${shellTokens.serif}`,
  color: "#ff9d7a",
});
export const priceField = style({
  position: "relative",
  display: "block",
});
export const priceCoin = style({
  position: "absolute",
  zIndex: 1,
  left: "14px",
  // Centred on the input row, not on the field with its message below.
  top: `calc(${t.size.compact} / 2)`,
  width: "26px",
  height: "26px",
  transform: "translateY(-50%)",
  pointerEvents: "none",
});
globalStyle(`${priceField} input`, { paddingLeft: "50px" });
/** An empty or quiet list keeps its message and action clear of a phone's dock. */
export const quiet = style({
  // The library's empty state sizes its words from here: a 28px title, as ML-10 shows it.
  fontSize: "12px",
  selectors: { [phone]: { paddingBottom: "130px" } },
});
