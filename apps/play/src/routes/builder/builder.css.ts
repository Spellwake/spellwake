import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import {
  glassEdge,
  glassSurface,
  TOUCH_TARGET,
  touch,
} from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/** Pane breakpoints in design pixels, as rem so they follow display density. */
/**
 * Narrower than this, the builder shows one pane at a time with a switch
 * between them. There is no two-pane form: the inspector holds the count and
 * the move actions, so a layout that dropped it left cards uneditable.
 */
const ONE_PANE = "builder (inline-size < 75rem)";
/** Narrower than this, the deck pane has no room for the words on its view switch. */
const TIGHT_DECK = "deck (inline-size < 37rem)";
const arrive = cssEase(m.ease.arrive);
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "-2px",
} as const;
const selectedEdge = "#f2deb0";

export const page = style({
  // The panes and the switch between them answer to the same width.
  containerName: "builder",
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
});
export const wideOnly = style({
  display: "contents",
  selectors: { ':root[data-shell="compact"] &': { display: "none" } },
});
export const formatBox = style({ width: "204px", flexShrink: 0 });
/** Square icon action in the task header. */
export const iconOnly = style({ width: "46px", paddingInline: 0 });
globalStyle(`${iconOnly}`, {
  "@media": { [touch]: { width: `max(46px, ${TOUCH_TARGET})` } },
});

/** An action whose destination is not available. It keeps its place and explains itself on hover and focus. */
export const pendingAction = style({
  appearance: "none",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "10px",
  minHeight: t.size.compactAction,
  padding: "0 18px",
  borderRadius: t.size.radius,
  border: `1px solid ${t.color.formEdge}`,
  background: t.color.surface,
  color: shellTokens.ink,
  font: `${t.font.small}/1.1 ${shellTokens.serif}`,
  whiteSpace: "nowrap",
  opacity: 0.55,
  cursor: "default",
  selectors: { "&:focus-visible": { ...focus, outlineOffset: "2px" } },
});
globalStyle(`${pendingAction} svg`, { width: "22px", height: "22px" });

/* Workbench ---------------------------------------------------------------- */

export const workbench = style({
  flex: 1,
  minHeight: 0,
  display: "flex",
  padding: "32px 20px",
  "@container": { [ONE_PANE]: { padding: "16px 14px 12px" } },
});
globalStyle(`:root[data-shell="compact"] ${workbench}`, {
  padding: "16px 14px 12px",
});
/** Column proportions measured from the approved builder reference. */
export const panes = style({
  flex: 1,
  minWidth: 0,
  minHeight: 0,
  display: "grid",
  gridTemplateColumns: "370fr 642fr 519fr",
  gap: "8px",
  "@container": {
    [ONE_PANE]: { gridTemplateColumns: "minmax(0,1fr)" },
  },
});
export const pane = style({
  ...glassSurface,
  minWidth: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  borderRadius: "14px",
  border: `1px solid ${glassEdge}`,
  overflow: "hidden",
});
/** A narrow builder shows one pane at a time, the inspector included. */
export const inspectorPane = style({
  selectors: {
    '[data-pane]:not([data-pane="inspector"]) &': {
      "@container": { [ONE_PANE]: { display: "none" } },
    },
  },
});
export const deckPane = style({
  selectors: {
    '[data-pane]:not([data-pane="deck"]) &': {
      "@container": { [ONE_PANE]: { display: "none" } },
    },
  },
});
export const searchPane = style({
  selectors: {
    '[data-pane]:not([data-pane="search"]) &': {
      "@container": { [ONE_PANE]: { display: "none" } },
    },
  },
});
export const scroll = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  overflowX: "hidden",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  selectors: { "&:focus-visible": focus },
});
export const paneSwitch = style({
  ...glassSurface,
  flexShrink: 0,
  display: "none",
  margin: "0 14px",
  marginBottom: "max(14px, env(safe-area-inset-bottom, 0px))",
  padding: "6px",
  gap: "6px",
  borderRadius: "999px",
  border: `1px solid ${glassEdge}`,
  "@container": { [ONE_PANE]: { display: "flex" } },
});
export const paneChoice = style({
  appearance: "none",
  flex: 1,
  minHeight: `max(56px, ${TOUCH_TARGET})`,
  border: 0,
  borderRadius: "999px",
  background: "transparent",
  color: shellTokens.ink,
  font: `22px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&[aria-pressed=true]": {
      color: shellTokens.active,
      background: "linear-gradient(104deg,#5a4526b3,#3a3324a6 50%,#22302f99)",
      boxShadow: `inset 0 0 0 1px ${shellTokens.line}80`,
    },
    "&:focus-visible": { ...focus, outlineOffset: "2px" },
  },
});

/* Deck pane ---------------------------------------------------------------- */

export const deckHead = style({
  containerName: "deck",
  containerType: "inline-size",
  flexShrink: 0,
  display: "flex",
  flexDirection: "column",
  gap: "14px",
  padding: "14px 14px 12px",
});
export const deckControls = style({
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  gap: "12px",
});
export const viewToggle = style({
  display: "flex",
  marginRight: "auto",
  border: `1px solid ${t.color.formEdge}`,
  borderRadius: "8px",
  overflow: "hidden",
});
export const viewChoice = style({
  appearance: "none",
  display: "flex",
  alignItems: "center",
  gap: "10px",
  minHeight: "38px",
  padding: "0 18px",
  border: 0,
  background: "transparent",
  color: shellTokens.ink,
  font: `20px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
  selectors: {
    "&[aria-pressed=true]": {
      color: shellTokens.active,
      background: "#f6e9d31f",
      boxShadow: `inset 0 0 0 1px ${selectedEdge}`,
      borderRadius: "7px",
    },
    "&:focus-visible": focus,
  },
});
globalStyle(`${viewChoice} svg`, { width: "20px", height: "20px" });
// Where the deck pane is narrow (one pane at a time, or three panes on a
// tablet on its side) the two views are told apart by their icons, which
// leaves room for Group and Sort on the same row. The words stay as the
// buttons' names.
const iconChoice = {
  justifyContent: "center",
  minWidth: TOUCH_TARGET,
  padding: 0,
} as const;
globalStyle(`${viewChoice}`, {
  "@container": { [ONE_PANE]: iconChoice, [TIGHT_DECK]: iconChoice },
});
const hiddenLabel = {
  position: "absolute",
  width: "1px",
  height: "1px",
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
} as const;
export const viewLabel = style({
  "@container": { [ONE_PANE]: hiddenLabel, [TIGHT_DECK]: hiddenLabel },
});
export const sortBox = style({
  width: "170px",
  "@container": { [ONE_PANE]: { flex: "1 1 0", width: "auto", minWidth: 0 } },
});
// Three section names share a phone's width, so they are set a little tighter there.
globalStyle(`${deckHead} [role="tab"]`, {
  "@container": {
    [ONE_PANE]: {
      paddingInline: "4px",
      fontSize: "19px",
      letterSpacing: "-0.01em",
    },
  },
});
// The pane's own selects are a size down from page-level controls, as in the approved builder.
const compactSelect = {
  fontSize: "20px",
  minHeight: "38px",
  height: "38px",
  paddingInline: "12px",
  "@media": { [touch]: { height: TOUCH_TARGET } },
} as const;
globalStyle(`${sortBox} > button`, compactSelect);
export const deckScroll = style([scroll, { padding: "0 14px 16px" }]);
export const group = style({ marginBottom: "6px" });
export const groupHead = style({
  appearance: "none",
  display: "flex",
  alignItems: "center",
  gap: "12px",
  width: "100%",
  minHeight: "38px",
  padding: "0 12px",
  border: 0,
  borderRadius: "6px",
  background: "#f6e9d30f",
  color: shellTokens.ink,
  font: `23px/1.1 ${shellTokens.serif}`,
  textAlign: "left",
  cursor: "pointer",
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
  selectors: { "&:focus-visible": focus },
});
globalStyle(`${groupHead} svg`, {
  width: "20px",
  height: "20px",
  flexShrink: 0,
  "@media": {
    [standardMotion]: { transition: `rotate ${m.duration.settle}s ${arrive}` },
  },
});
globalStyle(`${groupHead}[aria-expanded=false] svg`, { rotate: "-90deg" });
export const groupCount = style({ marginLeft: "auto" });

const rowBase = {
  position: "relative",
  borderRadius: "8px",
  selectors: {
    "&[data-selected=true]": {
      background: "#f6e9d314",
      boxShadow: `inset 0 0 0 2px ${selectedEdge}`,
    },
    "&:hover": { background: "#f6e9d30f" },
  },
} as const;
/** The whole row selects its card. The button underlies the row so other controls can sit on top. */
export const rowSelect = style({
  appearance: "none",
  position: "absolute",
  inset: 0,
  border: 0,
  borderRadius: "inherit",
  background: "transparent",
  cursor: "pointer",
  selectors: { "&:focus-visible": focus },
});
export const listRow = style({
  ...rowBase,
  display: "grid",
  gridTemplateColumns: "34px 66px minmax(0,1.15fr) minmax(0,1fr) auto",
  alignItems: "center",
  gap: "12px",
  minHeight: "46px",
  padding: "0 12px",
  font: `21px/1.15 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
  // One narrow pane gives the name the room the type line would take.
  "@container": {
    [ONE_PANE]: { gridTemplateColumns: "34px 66px minmax(0,1fr) auto" },
  },
});
globalStyle(`${listRow} > *:not(button), ${listRow} > *:not(button) *`, {
  pointerEvents: "none",
});
export const quantity = style({
  textAlign: "right",
  fontVariantNumeric: "tabular-nums",
});
export const thumb = style({
  width: "66px",
  height: "36px",
  objectFit: "cover",
  borderRadius: "4px",
});
export const cardName = style({
  minWidth: 0,
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
});
export const cardType = style({
  "@container": { [ONE_PANE]: { display: "none" } },
  minWidth: 0,
  fontSize: "16px",
  color: t.color.muted,
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
});
export const cost = style({
  display: "inline-flex",
  gap: "2px",
  minWidth: "62px",
});
globalStyle(`${cost} img`, { width: "20px", height: "20px" });
/** Known format violation. The reason is in its tooltip, on hover and on focus. */
export const invalid = style({
  position: "relative",
  zIndex: 1,
  pointerEvents: "auto",
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "24px",
  height: "24px",
  padding: 0,
  border: "1px solid #ff9d8f",
  borderRadius: "50%",
  background: "#5c2824",
  color: "#ffd9d2",
  font: "700 15px/1 system-ui, sans-serif",
  cursor: "help",
  "@media": { [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET } },
});
export const rowEnd = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "flex-end",
  gap: "8px",
});
globalStyle(`${rowEnd} > button`, { pointerEvents: "auto" });

/** Cards view: named art strips with counts, following the approved Cards reference. */
export const tiles = style({
  columnCount: 2,
  columnGap: "14px",
  "@container": { [ONE_PANE]: { columnCount: 1 } },
});
export const tileGroup = style({ breakInside: "avoid", marginBottom: "14px" });
export const tile = style({
  ...rowBase,
  height: "102px",
  marginTop: "6px",
  overflow: "hidden",
  border: "1px solid #f6e9d359",
  backgroundSize: "cover",
  backgroundPosition: "center 28%",
  selectors: {
    "&[data-selected=true]": {
      boxShadow: `inset 0 0 0 2px ${selectedEdge}, 0 0 14px #f2c87555`,
    },
  },
});
export const tileBar = style({
  position: "absolute",
  insetInline: 0,
  top: 0,
  display: "flex",
  alignItems: "center",
  gap: "8px",
  minHeight: "36px",
  padding: "0 8px 0 12px",
  background: "linear-gradient(180deg,#0c1118e6,#0c111899 70%,transparent)",
  font: `20px/1.1 ${shellTokens.serif}`,
  color: shellTokens.ink,
  pointerEvents: "none",
});
export const tileCount = style({
  marginLeft: "auto",
  display: "grid",
  placeItems: "center",
  minWidth: "28px",
  height: "28px",
  padding: "0 6px",
  borderRadius: "999px",
  background: "#0c1118cc",
  border: "1px solid #f6e9d359",
  fontVariantNumeric: "tabular-nums",
});
globalStyle(`${tileBar} > button`, { pointerEvents: "auto" });

/* Search pane -------------------------------------------------------------- */

export const searchHead = style({
  flexShrink: 0,
  display: "flex",
  flexDirection: "column",
  gap: "14px",
  padding: "20px 18px 14px",
});
export const filters = style({ display: "flex", gap: "12px" });
export const filterBox = style({ width: "152px" });
globalStyle(`${filterBox} > button`, compactSelect);
export const searchScroll = style([scroll, { padding: "0 18px 16px" }]);
export const result = style({
  ...rowBase,
  display: "grid",
  gridTemplateColumns: "84px minmax(0,1fr) auto auto",
  alignItems: "center",
  gap: "16px",
  minHeight: "87px",
  padding: "8px 6px 8px 0",
  borderRadius: 0,
  borderBottom: `1px solid ${shellTokens.divider}`,
  color: shellTokens.ink,
});
globalStyle(`${result} > *:not(button), ${result} > *:not(button) *`, {
  pointerEvents: "none",
});
export const resultArt = style({
  width: "84px",
  height: "70px",
  objectFit: "cover",
  borderRadius: "6px",
});
export const resultName = style({
  font: `23px/1.2 ${shellTokens.serif}`,
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
});
export const resultType = style({ fontSize: "18px", color: t.color.muted });
export const add = style({
  position: "relative",
  zIndex: 1,
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "44px",
  height: "44px",
  padding: 0,
  borderRadius: "8px",
  border: `1px solid ${t.color.formEdge}`,
  background: t.color.surface,
  color: shellTokens.ink,
  cursor: "pointer",
  "@media": { [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET } },
  selectors: {
    "&:hover": { color: shellTokens.active, borderColor: selectedEdge },
    "&:focus-visible": { ...focus, outlineOffset: "2px" },
    "&[aria-disabled=true]": { opacity: 0.45, cursor: "default" },
  },
});
globalStyle(`${add} svg`, { width: "22px", height: "22px" });
export const quiet = style({
  display: "grid",
  placeItems: "center",
  minHeight: "160px",
  padding: "24px",
  textAlign: "center",
});

/* Inspector ---------------------------------------------------------------- */

export const inspector = style([
  scroll,
  {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: "22px",
    padding: "44px 26px 26px",
    "@container": { [ONE_PANE]: { padding: "20px 18px 18px" } },
  },
]);
export const face = style({
  width: "100%",
  maxWidth: "320px",
  "@container": {
    // On a phone the card is the pane: it grows to the height left over once
    // the header, the count, the moves and the pane switch have their room,
    // and never past the pane's width.
    [ONE_PANE]: {
      maxWidth: "min(100%, max(320px, calc((100dvh - 330PX) * 488 / 680)))",
    },
  },
  aspectRatio: "488 / 680",
  borderRadius: "4.6% / 3.3%",
  boxShadow: "0 14px 34px #000000a6",
});
export const moves = style({
  display: "flex",
  flexDirection: "column",
  gap: "16px",
  width: "100%",
  maxWidth: "308px",
});
globalStyle(`${moves} > button`, { width: "100%" });
/** The card the deck box shows, said quietly where its button would be. */
export const faceCard = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "8px",
  minHeight: t.size.compactAction,
  margin: 0,
  color: t.color.muted,
  font: `20px/1.2 ${shellTokens.serif}`,
});
globalStyle(`${faceCard} svg`, { width: "18px", height: "18px" });

/** The two section moves sit side by side, as in the approved inspector. */
export const movePair = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
  gap: "16px",
});
globalStyle(`${movePair} > button`, { width: "100%", paddingInline: "8px" });
export const fieldError = style({
  margin: "-12px 0 0",
  fontSize: "20px",
  color: "#ffb4a8",
});
/** Room for one line of feedback under the list, held open so the dialog does not jump. */
export const exportNotice = style({ minHeight: "44px", marginTop: "12px" });
export const exportText = style({
  display: "block",
  width: "100%",
  minHeight: "210px",
  maxHeight: "44vh",
  resize: "none",
  padding: "16px 18px",
  borderRadius: "10px",
  border: `1px solid ${t.color.formEdge}`,
  background: "#05090fb3",
  color: shellTokens.ink,
  font: `24px/1.5 ${shellTokens.serif}`,
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  selectors: { "&:focus-visible": focus },
});
