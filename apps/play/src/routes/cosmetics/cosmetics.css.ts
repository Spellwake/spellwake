import { createVar, globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/**
 * The cosmetic picker shared by Default settings (DS-01 to DS-03) and
 * Cosmetics (CC-01 to CC-05): categories, a gallery of choices, and a preview.
 * Lengths are in the pixel scale of the approved references. Narrower than
 * `STACKED` the three columns become three rows: categories, preview, gallery.
 */
const STACKED = "picker (inline-size < 46rem)";
const arrive = cssEase(m.ease.arrive);
const rule = "#a698774c";
const selectedEdge = "#f2deb0";
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;

/** How an item is painted. Set per element, so one class draws every item. */
export const look = createVar();

/** The dialog, at the approved width. */
export const shell = style({
  selectors: { '&&:not([data-presentation="drawer"])': { width: "1180px" } },
});
export const layout = style({
  containerName: "picker",
  containerType: "inline-size",
  position: "relative",
  display: "flex",
  flexDirection: "column",
  minHeight: "492px",
  "@container": { [STACKED]: { minHeight: 0 } },
});
export const tabs = style({
  margin: "-8px 0 16px",
  borderBottom: `1px solid ${rule}`,
});
/** Your cosmetics and Shop: two compact boxed tabs standing on the rule. */
export const viewTabs = style({ display: "flex", gap: "12px" });
export const viewTab = style({
  appearance: "none",
  minHeight: "46px",
  marginBottom: "-1px",
  padding: "0 30px",
  border: "1px solid #5b5a55",
  borderBottomColor: rule,
  borderRadius: "10px 10px 0 0",
  background: "#0e141a",
  color: "#d8cdb9",
  font: `24px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&:hover": { color: shellTokens.ink },
    "&:focus-visible": focus,
    '&[aria-selected="true"]': {
      color: shellTokens.active,
      borderColor: selectedEdge,
      background: "linear-gradient(180deg,#2e2a22,#1d1b18)",
    },
  },
  "@media": { [touch]: { minHeight: `max(46px, ${TOUCH_TARGET})` } },
  "@container": { [STACKED]: { flex: 1, padding: "0 12px", fontSize: "21px" } },
});
export const columns = style({
  flex: 1,
  minHeight: 0,
  display: "grid",
  gridTemplateColumns: "232px 1px minmax(0, 1.45fr) 1px minmax(0, 1fr)",
  "@container": {
    [STACKED]: {
      gridTemplateColumns: "minmax(0, 1fr)",
      gridTemplateAreas: '"nav" "preview" "gallery"',
      gap: "16px",
    },
  },
});
export const divider = style({
  background: rule,
  "@container": { [STACKED]: { display: "none" } },
});

/* Categories */
export const categories = style({
  position: "relative",
  display: "flex",
  flexDirection: "column",
  gap: "10px",
  padding: "8px 18px 8px 0",
  "@container": {
    [STACKED]: {
      gridArea: "nav",
      flexDirection: "row",
      flexWrap: "wrap",
      gap: "6px",
      padding: 0,
    },
  },
});
export const category = style({
  appearance: "none",
  position: "relative",
  flex: "none",
  display: "flex",
  alignItems: "center",
  gap: "16px",
  minHeight: "57px",
  padding: "0 18px",
  border: "1px solid transparent",
  borderRadius: "8px",
  background: "transparent",
  color: shellTokens.ink,
  font: `25px/1.2 ${shellTokens.serif}`,
  textAlign: "start",
  whiteSpace: "nowrap",
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#ffffff0a" },
    "&:focus-visible": focus,
    '&[aria-selected="true"]': { color: shellTokens.active },
  },
  "@media": { [touch]: { minHeight: `max(57px, ${TOUCH_TARGET})` } },
  "@container": {
    [STACKED]: { minHeight: "48px", gap: "10px", padding: "0 14px" },
  },
});
globalStyle(`${category} svg`, {
  position: "relative",
  zIndex: 1,
  flex: "none",
  width: "30px",
  height: "30px",
  strokeWidth: 1.3,
});
globalStyle(`${category} svg`, {
  "@container": { [STACKED]: { width: "22px", height: "22px" } },
});
/** The filled plate that travels to the selected category. */
export const categoryPlate = style({
  position: "absolute",
  inset: -1,
  zIndex: 0,
  border: "1px solid #b9a67d",
  borderRadius: "8px",
  background: "linear-gradient(100deg,#37322a,#2a2722)",
  boxShadow: "inset 0 1px 0 #ffffff12",
});
export const categoryLabel = style({ position: "relative", zIndex: 1 });

/* Gallery */
export const gallery = style({
  minWidth: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  gap: "16px",
  padding: "8px 26px",
  "@container": { [STACKED]: { gridArea: "gallery", padding: 0 } },
});
export const grid = style({
  flex: "1 1 0",
  minHeight: "210px",
  overflowY: "auto",
  display: "grid",
  gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
  alignContent: "start",
  gap: "20px 22px",
  // Room for the selected tile's outline inside the scroller.
  margin: "-4px",
  padding: "4px",
  scrollbarWidth: "thin",
  scrollbarColor: "#8d8168 transparent",
  "@container": {
    // In a drawer the whole body scrolls, so the gallery does not scroll within it.
    [STACKED]: {
      flex: "none",
      minHeight: 0,
      overflowY: "visible",
      gap: "16px 14px",
      // As many tiles across as the width holds: three on a phone, more on a tablet.
      gridTemplateColumns: "repeat(auto-fill, minmax(120px, 1fr))",
    },
  },
});
export const tile = style({
  appearance: "none",
  display: "grid",
  justifyItems: "center",
  gap: "8px",
  padding: 0,
  border: 0,
  background: "transparent",
  color: shellTokens.ink,
  font: `21px/1.2 ${shellTokens.serif}`,
  textAlign: "center",
  cursor: "pointer",
  selectors: { "&:focus-visible": { outline: "none" } },
});
export const tileArt = style({
  position: "relative",
  display: "grid",
  placeItems: "center",
  width: "100%",
  borderRadius: "9px",
  outline: "2px solid transparent",
  outlineOffset: "2px",
  selectors: {
    [`${tile}:hover &`]: { outlineColor: "#f2deb066" },
    [`${tile}:focus-visible &`]: { outlineColor: t.color.focus },
    [`${tile}[aria-pressed="true"] &`]: { outlineColor: selectedEdge },
  },
  "@media": {
    [standardMotion]: {
      transition: `outline-color ${m.duration.feedback}s ${arrive}`,
    },
  },
});
export const tileName = style({
  maxWidth: "100%",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const tilePrice = style({ marginTop: "-4px", color: "#e9dcc0" });
/** The tick on the chosen default. */
export const tick = style({
  position: "absolute",
  top: "8px",
  right: "8px",
  display: "grid",
  placeItems: "center",
  width: "26px",
  height: "26px",
  borderRadius: "50%",
  background: "#f6e9d3",
  color: "#1c1508",
  boxShadow: "0 2px 6px #00000080",
});
globalStyle(`${tick} svg`, { width: "17px", height: "17px", strokeWidth: 2.6 });
export const empty = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  minHeight: "210px",
  margin: 0,
  font: `italic 25px/1.3 ${shellTokens.serif}`,
  color: "#d8cdb9",
});

/* How each kind of item is drawn, in a tile and in the preview. */
const sheen =
  "linear-gradient(135deg, #ffffff24, transparent 34%, transparent 66%, #00000038)";
export const card = style({
  display: "block",
  width: "100%",
  aspectRatio: "5 / 7",
  borderRadius: "7% / 5%",
  border: "1px solid #ffffff38",
  background: `${sheen}, ${look}`,
  backgroundSize: "cover",
  backgroundPosition: "center",
  boxShadow: "0 6px 18px #00000080",
});
export const die = style({
  position: "relative",
  display: "block",
  width: "62%",
  aspectRatio: "1",
  margin: "19% 0",
  borderRadius: "20%",
  border: "1px solid #ffffff40",
  background: `${sheen}, ${look}`,
  boxShadow: "0 8px 20px #00000099, inset 0 -6px 12px #00000040",
});
export const pip = style({
  position: "absolute",
  width: "16%",
  height: "16%",
  borderRadius: "50%",
  background: "#dcdcdc",
  // Dark on a light die and light on a dark one, whatever it is painted with.
  mixBlendMode: "difference",
  transform: "translate(-50%, -50%)",
});
export const mat = style({
  display: "block",
  width: "100%",
  aspectRatio: "16 / 10",
  margin: "20% 0",
  borderRadius: "5%",
  border: "1px solid #ffffff38",
  background: `${sheen}, ${look}`,
  backgroundSize: "cover",
  backgroundPosition: "center",
  boxShadow: "0 6px 18px #00000080",
});
/** A choice with nothing to draw: None, or an item whose artwork the source lacks. */
export const blank = style({
  display: "grid",
  placeItems: "center",
  width: "100%",
  aspectRatio: "5 / 7",
  borderRadius: "7% / 5%",
  border: "1px solid #ffffff2e",
  background: "linear-gradient(160deg,#141c26,#0b1118)",
  color: "#8f8a7c",
});
globalStyle(`${blank} svg`, { width: "34%", height: "34%", strokeWidth: 1.1 });

/** Live procedural wear preview frame for card & sleeve condition effects. */
export const wearFrame = style({
  display: "grid",
  placeItems: "center",
  width: "100%",
  aspectRatio: "5 / 7",
  borderRadius: "7% / 5%",
  border: "1px solid #ffffff2e",
  background: "radial-gradient(circle at 50% 45%, #1c2430, #0d1219 78%)",
  boxShadow: "0 6px 18px #00000080",
  overflow: "hidden",
});
export const wearCanvas = style({
  display: "block",
  width: "100%",
  height: "100%",
  objectFit: "contain",
});

/** Crisp 16:10 playmat frame with custom merrowed or saddle-stitched edges (no surface sheen). */
export const playmatFrame = style({
  display: "block",
  width: "100%",
  aspectRatio: "16 / 10",
  margin: "20% 0",
  borderRadius: "5%",
  overflow: "hidden",
  background: "#10141a",
  boxShadow: "0 6px 18px #00000080, inset 0 0 0 1px #ffffff1a",
});
export const playmatCanvas = style({
  display: "block",
  width: "100%",
  height: "100%",
});

// A gallery tile is a little squarer than a card, as in the approved walkthrough.
globalStyle(
  `${tileArt} > ${card}, ${tileArt} > ${blank}, ${tileArt} > ${wearFrame}`,
  {
    aspectRatio: "8 / 9",
    borderRadius: "9px",
  },
);

/* Preview */
export const preview = style({
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  gap: "10px",
  padding: "8px 0 8px 26px",
  textAlign: "center",
  "@container": {
    [STACKED]: {
      gridArea: "preview",
      flexDirection: "row",
      justifyContent: "flex-start",
      gap: "20px",
      padding: "14px 16px",
      border: `1px solid ${rule}`,
      borderRadius: "12px",
      background: "#0a1016",
      textAlign: "start",
    },
  },
});
// With no caption beside it, as in Default settings, the preview centres its item.
globalStyle(`${preview}:not(:has(> div))`, {
  "@container": { [STACKED]: { justifyContent: "center" } },
});
export const stage = style({
  position: "relative",
  flex: "none",
  display: "grid",
  placeItems: "center",
  width: "100%",
  height: "300px",
  "@container": { [STACKED]: { width: "150px", height: "132px" } },
});
/** The soft pool of light the previewed item stands in. */
export const floor = style({
  position: "absolute",
  insetInline: "6%",
  bottom: "4%",
  height: "26%",
  borderRadius: "50%",
  background: "radial-gradient(closest-side, #5d7fb226, transparent)",
  pointerEvents: "none",
});
const settle = keyframes({
  from: { opacity: 0, transform: "translateY(10px) scale(0.97)" },
});
export const shown = style({
  position: "relative",
  display: "grid",
  placeItems: "center",
  width: "100%",
  height: "100%",
  "@media": {
    [standardMotion]: {
      animation: `${settle} ${m.duration.scene}s ${arrive} both`,
    },
  },
});
/** Three sleeves, fanned. The outer two open from behind the middle one. */
export const fan = style({
  position: "relative",
  width: "34%",
  aspectRatio: "5 / 7",
});
const spread = keyframes({ from: { transform: "rotate(0deg) translateX(0)" } });
export const fanned = style({
  position: "absolute",
  inset: 0,
  transformOrigin: "50% 120%",
  selectors: {
    '&[data-side="left"]': { transform: "rotate(-12deg) translateX(-58%)" },
    '&[data-side="right"]': { transform: "rotate(12deg) translateX(58%)" },
  },
  "@media": {
    [standardMotion]: {
      animation: `${spread} ${m.duration.hero * 0.7}s ${arrive} both`,
    },
  },
});
export const single = style({ width: "42%" });
export const bigDie = style({ width: "44%" });
export const bigMat = style({ width: "88%" });
export const details = style({
  // Room for the price and Buy, so the dialog keeps its size when they become Owned.
  minHeight: "192px",
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  alignItems: "inherit",
  gap: "6px",
  "@container": {
    [STACKED]: { alignItems: "flex-start", flex: 1, minHeight: 0 },
  },
});
export const itemName = style({
  margin: 0,
  maxWidth: "100%",
  font: `36px/1.15 ${shellTokens.serif}`,
  fontWeight: 600,
  overflowWrap: "anywhere",
  "@container": { [STACKED]: { fontSize: "28px" } },
});
export const itemKind = style({
  margin: 0,
  font: `italic 23px/1.2 ${shellTokens.serif}`,
  color: "#e9dcc0",
});
export const itemPrice = style({
  margin: "4px 0 0",
  font: `30px/1.2 ${shellTokens.serif}`,
  "@container": { [STACKED]: { fontSize: "24px" } },
});
export const buy = style({
  marginTop: "12px",
  selectors: {
    "&&": { minWidth: "230px", minHeight: "58px", fontSize: "27px" },
  },
  "@container": {
    [STACKED]: {
      marginTop: "8px",
      selectors: {
        "&&": { minWidth: "150px", minHeight: "50px", fontSize: "23px" },
      },
    },
  },
});
/** Owned is a status, not a control. */
export const owned = style({
  marginTop: "12px",
  display: "grid",
  placeItems: "center",
  minWidth: "210px",
  minHeight: "46px",
  padding: "0 24px",
  borderRadius: "999px",
  border: "1px solid #8d8168",
  font: `25px/1 ${shellTokens.serif}`,
  color: "#e9dcc0",
  "@container": {
    [STACKED]: {
      marginTop: "8px",
      minWidth: 0,
      minHeight: "40px",
      fontSize: "21px",
    },
  },
});

/** A lettered action of the dialog footer. */
export const action = style({
  selectors: { "&&": { minHeight: "55px", padding: "0 38px" } },
  "@media": {
    [touch]: {
      selectors: { "&&": { minHeight: `max(55px, ${TOUCH_TARGET})` } },
    },
  },
});
/** One line of passive feedback, held in the dialog's lower corner. */
export const notice = style({
  position: "absolute",
  left: 0,
  bottom: 0,
  width: "min(100%, 520px)",
  pointerEvents: "none",
  "@container": { [STACKED]: { position: "static", minHeight: "56px" } },
});
export const skeleton = style({ display: "contents" });

/** Pattern seed indicator and reroll controls under a wear effect preview. */
export const seedControls = style({
  marginTop: "8px",
  display: "flex",
  flexDirection: "column",
  alignItems: "inherit",
  gap: "6px",
});
export const seedTag = style({
  fontSize: "13px",
  letterSpacing: "0.03em",
  color: "#c8bfa8",
});
export const seedActions = style({
  display: "flex",
  flexWrap: "wrap",
  justifyContent: "center",
  gap: "8px",
});
export const seedButton = style({
  appearance: "none",
  display: "inline-flex",
  alignItems: "center",
  gap: "6px",
  minHeight: "34px",
  padding: "0 14px",
  borderRadius: "999px",
  border: "1px solid #8d8168",
  background: "linear-gradient(180deg,#25221d,#171614)",
  color: shellTokens.ink,
  fontSize: "14px",
  cursor: "pointer",
  selectors: {
    "&:hover": {
      borderColor: selectedEdge,
      color: shellTokens.active,
    },
    "&:focus-visible": focus,
  },
});
globalStyle(`${seedButton} svg`, { width: "14px", height: "14px" });
