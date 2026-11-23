import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/**
 * Collection Cards, as approved (CLC-01 to CLC-08): a gallery with an
 * inspector on its right. Lengths are in the pixel scale of the approved
 * references. The compact frame (phones and upright tablets) drops the side
 * inspector; a chosen card opens it in a dialog instead.
 */
const compact = ':root[data-shell="compact"] &';
const phone = ':root[data-device="phone"] &';
const phoneLandscape =
  ':root[data-device="phone"][data-orientation="landscape"] &';
const arrive = cssEase(m.ease.arrive);
const champagne = "#e7cf9f";
const rule = "#a698774c";
const selectedEdge = "#f2deb0";
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "3px",
} as const;
/** Toolbar breakpoints by the gallery's own width, in design pixels as rem. */
const TOOLBAR_TWO_ROWS = "cards (inline-size < 46rem)";

export const page = style({
  position: "relative",
  flex: 1,
  minHeight: 0,
  display: "grid",
  gridTemplateColumns: "minmax(0,1fr) 372px",
  selectors: { [compact]: { gridTemplateColumns: "minmax(0,1fr)" } },
});
export const main = style({
  containerName: "cards",
  containerType: "inline-size",
  minWidth: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  padding: "36px 32px 0 37px",
  selectors: { [phone]: { padding: "22px 22px 0" } },
});

export const toolbar = style({
  position: "relative",
  zIndex: 2,
  flexShrink: 0,
  display: "grid",
  gridTemplateColumns: "minmax(0,432px) 204px minmax(216px,max-content)",
  alignItems: "center",
  gap: "36px",
  "@container": {
    [TOOLBAR_TWO_ROWS]: {
      gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
      gap: "14px",
    },
  },
});
export const search = style({
  minWidth: 0,
  "@container": { [TOOLBAR_TWO_ROWS]: { gridColumn: "1 / -1" } },
});
globalStyle(`${toolbar} > *`, { minWidth: 0 });
globalStyle(`${toolbar} > button`, { width: "100%", whiteSpace: "nowrap" });

/** The Filters trigger: a field-like button with its count and a chevron. */
export const filtersButton = style({
  appearance: "none",
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "12px",
  height: t.size.compact,
  padding: "0 22px 0 24px",
  border: `1px solid ${t.color.formEdge}`,
  borderRadius: "10px",
  background: "#0b1016cc",
  color: shellTokens.ink,
  font: `25px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#151b22" },
    "&:focus-visible": focus,
    '&[aria-expanded="true"]': { borderColor: t.color.goldEdge },
  },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const chevron = style({ width: "22px", height: "22px", flexShrink: 0 });

export const chips = style({
  flexShrink: 0,
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "14px 18px",
  paddingTop: "18px",
  selectors: {
    [phone]: {
      flexWrap: "nowrap",
      overflowX: "auto",
      scrollbarWidth: "none",
      marginInline: "-22px",
      paddingInline: "22px",
    },
  },
});
export const clearFilters = style({
  appearance: "none",
  marginLeft: "auto",
  padding: "6px 2px",
  border: 0,
  background: "none",
  color: champagne,
  font: `22px/1.2 ${shellTokens.serif}`,
  textDecoration: "underline",
  textUnderlineOffset: "4px",
  whiteSpace: "nowrap",
  cursor: "pointer",
  selectors: { "&:focus-visible": focus },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});

export const scroll = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  marginInline: "-20px",
  paddingInline: "20px",
  WebkitMaskImage:
    "linear-gradient(180deg,transparent 0,#000 30px,#000 calc(100% - 28px),transparent 100%)",
  maskImage:
    "linear-gradient(180deg,transparent 0,#000 30px,#000 calc(100% - 28px),transparent 100%)",
  selectors: {
    "&:focus-visible": {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "-2px",
    },
    [phone]: {
      marginInline: "-22px",
      paddingInline: "22px",
      scrollPaddingBottom: "140px",
    },
  },
});
/** Fixed tiles: wider windows gain columns instead of stretching the cards. */
export const grid = style({
  display: "grid",
  gridTemplateColumns: "repeat(auto-fill, 208px)",
  justifyContent: "start",
  gap: "30px 24px",
  padding: "34px 0 48px",
  listStyle: "none",
  margin: 0,
  selectors: {
    [compact]: {
      gridTemplateColumns: "repeat(auto-fill, minmax(190px, 1fr))",
      justifyContent: "stretch",
    },
    [phone]: {
      gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
      gap: "26px 18px",
      padding: "26px 0 150px",
    },
    [phoneLandscape]: {
      gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))",
      padding: "22px 0 40px",
    },
  },
});

/** One card: its face, its total count and caption, as one keyboard target. */
export const tile = style({
  appearance: "none",
  display: "flex",
  flexDirection: "column",
  alignItems: "stretch",
  gap: "12px",
  width: "100%",
  padding: 0,
  border: 0,
  background: "none",
  color: shellTokens.ink,
  textAlign: "left",
  cursor: "pointer",
  selectors: { "&:focus-visible": { outline: "none" } },
});
export const tileFace = style({
  position: "relative",
  display: "block",
  padding: "6px",
  borderRadius: "16px",
  border: "2px solid transparent",
  selectors: {
    [`${tile}[aria-pressed="true"] &`]: {
      borderColor: selectedEdge,
      boxShadow: "0 0 28px #f2deb02e",
    },
    [`${tile}:focus-visible &`]: {
      outline: `2px solid ${t.color.focus}`,
      outlineOffset: "3px",
    },
  },
  "@media": {
    // Motion only where the person has not asked for less.
    [standardMotion]: {
      transition: `border-color ${m.duration.settle}s ${arrive}, transform ${m.duration.settle}s ${arrive}, box-shadow ${m.duration.settle}s ${arrive}`,
      selectors: { [`${tile}:hover &`]: { transform: "translateY(-3px)" } },
    },
  },
});
export const count = style({
  position: "absolute",
  right: "-4px",
  bottom: "-6px",
  minWidth: "44px",
  padding: "4px 10px 5px",
  borderRadius: "10px",
  border: `1px solid ${champagne}`,
  background: "#0b0f14",
  color: shellTokens.ink,
  font: `21px/1 ${shellTokens.serif}`,
  textAlign: "center",
  boxShadow: "0 4px 12px #0009",
});
export const caption = style({
  display: "flex",
  flexDirection: "column",
  gap: "4px",
  paddingInline: "14px",
});
export const name = style({
  font: `25px/1.15 ${shellTokens.serif}`,
  overflowWrap: "anywhere",
});
export const printing = style({
  font: `20px/1.2 ${shellTokens.serif}`,
  color: champagne,
});

/** A card face, at the printed proportions. */
export const face = style({
  position: "relative",
  display: "block",
  width: "100%",
  aspectRatio: "63 / 88",
  borderRadius: "4.6% / 3.3%",
  overflow: "hidden",
  background: "#10151c",
  boxShadow: "0 10px 26px #000a",
});
export const faceImage = style({
  display: "block",
  width: "100%",
  height: "100%",
  objectFit: "cover",
});
/** Where artwork is missing, loading or unavailable: a quiet card-sized frame. */
export const placeholder = style({
  position: "absolute",
  inset: 0,
  display: "grid",
  placeContent: "center",
  justifyItems: "center",
  gap: "12px",
  padding: "12%",
  border: "1px solid #ffffff1f",
  borderRadius: "inherit",
  background:
    "radial-gradient(120% 70% at 50% 20%, #24303d, transparent 70%), linear-gradient(170deg, #1a222c, #0c1117)",
  color: "#d8ccb7",
  textAlign: "center",
  font: `italic 22px/1.25 ${shellTokens.serif}`,
});
export const placeholderIcon = style({
  width: "36%",
  height: "auto",
  opacity: 0.55,
});
export const placeholderName = style({
  fontStyle: "normal",
  color: shellTokens.ink,
  overflowWrap: "anywhere",
});

/** The inspector: the chosen card large, its name and count, and its owned printings. */
export const inspector = style({
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  gap: "0",
  padding: "40px 26px 24px 32px",
  borderLeft: `1px solid ${rule}`,
  selectors: {
    [compact]: { display: "none" },
    "&[data-covered]": { visibility: "hidden" },
  },
});
export const inspectorBody = style({
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
});
export const inspectorFace = style({
  position: "relative",
  width: "268px",
  maxWidth: "100%",
  flexShrink: 0,
  selectors: {
    [`${inspectorBody}[data-in-dialog] &`]: {
      width: "min(62%, 300px)",
      margin: "0 auto",
    },
  },
});
export const flip = style({ marginTop: "16px", alignSelf: "flex-start" });
export const inspectorName = style({
  margin: "30px 0 0",
  font: `40px/1.12 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  overflowWrap: "anywhere",
  selectors: {
    [`${inspectorBody}[data-in-dialog] &`]: { display: "none" },
  },
});
export const copies = style({
  margin: "6px 0 0",
  font: `30px/1.2 ${shellTokens.serif}`,
  color: champagne,
  selectors: {
    [`${inspectorBody}[data-in-dialog] &`]: { display: "none" },
  },
});
export const printingsHeading = style({
  display: "flex",
  alignItems: "center",
  gap: "18px",
  margin: "26px 0 12px",
  font: `23px/1.2 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  selectors: {
    "&::after": {
      content: '""',
      flex: 1,
      height: "1px",
      background: rule,
    },
  },
});
export const rows = style({
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  gap: "2px",
  margin: 0,
  padding: "2px 10px 2px 2px",
  marginRight: "-10px",
  overflowY: "auto",
  maxHeight: "320px",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
});
export const row = style({
  appearance: "none",
  display: "grid",
  gridTemplateColumns: "16px 58px minmax(0,1fr) max-content",
  alignItems: "center",
  gap: "0 10px",
  width: "100%",
  minHeight: "70px",
  padding: "6px 10px 6px 6px",
  border: "1.5px solid transparent",
  borderBottomColor: rule,
  borderRadius: "10px",
  background: "none",
  color: shellTokens.ink,
  textAlign: "left",
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#ffffff08" },
    "&:focus-visible": focus,
    '&[aria-checked="true"]': {
      borderColor: selectedEdge,
      background: "#ffffff06",
    },
  },
  "@media": { [touch]: { minHeight: `max(70px, ${TOUCH_TARGET})` } },
});
export const tick = style({ width: "16px", height: "16px", color: champagne });
/** The printing's artwork: the top of its face, as a small square. */
export const thumb = style({
  width: "58px",
  height: "52px",
  borderRadius: "4px",
  objectFit: "cover",
  objectPosition: "50% 22%",
  background: "#1a222c",
});
export const thumbBlank = style({
  width: "58px",
  height: "52px",
  borderRadius: "4px",
  border: "1px solid #ffffff1f",
  background: "linear-gradient(170deg, #1d2631, #0f141a)",
});
export const rowText = style({
  display: "flex",
  flexDirection: "column",
  gap: "2px",
  minWidth: 0,
  font: `21px/1.15 ${shellTokens.serif}`,
  whiteSpace: "nowrap",
});
export const rowCopies = style({
  font: `21px/1.15 ${shellTokens.serif}`,
  whiteSpace: "nowrap",
});

export const center = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  paddingBottom: "8%",
});
export const emptyIcon = style({
  width: "48px",
  height: "48px",
  color: champagne,
  marginBottom: "4px",
});
const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  width: "38px",
  height: "38px",
  borderRadius: "50%",
  border: "3px solid #ffffff26",
  borderTopColor: "#e9dcc4",
  "@media": { [standardMotion]: { animation: `${spin} 1s linear infinite` } },
});
export const smallSpinner = style([spinner, { width: "24px", height: "24px" }]);
export const loading = style({
  display: "grid",
  justifyItems: "center",
  gap: "20px",
  font: `27px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
/** An empty state with its icon, at the empty state's own type sizes. */
export const empty = style({
  display: "grid",
  justifyItems: "center",
  gap: "8px",
});
export const appending = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "14px",
  padding: "0 0 40px",
  font: `22px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const artStatus = style({
  position: "absolute",
  inset: 0,
  display: "grid",
  placeContent: "center",
  justifyItems: "center",
  gap: "16px",
  background: "#0d1218e6",
  color: shellTokens.ink,
  font: `22px/1.2 ${shellTokens.serif}`,
});

/** Filters: a bounded panel over the inspector, as approved (CLC-03). */
export const panel = style({
  position: "absolute",
  zIndex: 5,
  top: "10px",
  right: "12px",
  bottom: "24px",
  width: "396px",
  display: "flex",
  flexDirection: "column",
  border: `1.5px solid ${champagne}`,
  borderRadius: "14px",
  background: "#0b1016",
  boxShadow: "0 24px 60px #000c",
});
export const panelHead = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "20px 18px 14px 26px",
  borderBottom: `1px solid ${rule}`,
});
export const panelTitle = style({
  margin: 0,
  font: `34px/1 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
});
export const close = style({
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "44px",
  height: "44px",
  border: 0,
  borderRadius: "10px",
  background: "#ffffff12",
  color: shellTokens.ink,
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#ffffff1f" },
    "&:focus-visible": focus,
  },
});
export const panelScroll = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: "4px 26px 10px",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
});
export const panelFoot = style({
  padding: "18px 26px 22px",
  borderTop: `1px solid ${rule}`,
});
globalStyle(`${panelFoot} > button`, { width: "100%" });

export const facet = style({
  display: "flex",
  flexDirection: "column",
  gap: "12px",
  padding: "18px 0",
  borderBottom: `1px solid ${rule}`,
  selectors: { "&:last-child": { borderBottom: 0 } },
});
export const facetHead = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "12px",
});
export const facetLabel = style({
  margin: 0,
  font: `25px/1.1 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
});
// Menus in a facet (Format, Finish) take its width; the colour symbols keep theirs.
globalStyle(`${facet} > div > button[role="combobox"]`, { width: "100%" });
export const setList = style({
  display: "flex",
  flexDirection: "column",
  gap: "6px",
  maxHeight: "176px",
  overflowY: "auto",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
});
export const setOption = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "10px",
});
export const setCode = style({
  font: `20px/1 ${shellTokens.serif}`,
  color: champagne,
});
export const quiet = style({
  margin: 0,
  font: `italic 21px/1.3 ${shellTokens.serif}`,
  color: "#cfc4b1",
});

/** Color or Color identity: two segments, the chosen one filled. */
export const segments = style({
  display: "grid",
  gridTemplateColumns: "auto auto",
  padding: "3px",
  border: `1px solid ${t.color.formEdge}`,
  borderRadius: "10px",
  background: "#0b1016",
});
export const segment = style({
  appearance: "none",
  minHeight: "40px",
  padding: "0 12px",
  border: 0,
  borderRadius: "8px",
  background: "none",
  color: shellTokens.ink,
  font: `19px/1 ${shellTokens.serif}`,
  whiteSpace: "nowrap",
  cursor: "pointer",
  selectors: {
    "&:focus-visible": focus,
    '&[aria-checked="true"]': {
      background: "linear-gradient(180deg,#f1dcae,#d9bd85)",
      color: "#1b1710",
    },
  },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const symbols = style({ display: "flex", gap: "14px" });
export const symbol = style({
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "46px",
  height: "46px",
  padding: "3px",
  border: "2px solid transparent",
  borderRadius: "50%",
  background: "none",
  cursor: "pointer",
  opacity: 0.6,
  selectors: {
    "&:hover": { opacity: 0.85 },
    "&:focus-visible": focus,
    '&[aria-pressed="true"]': { opacity: 1, borderColor: selectedEdge },
  },
  "@media": { [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET } },
});
globalStyle(`${symbol} img`, { width: "100%", height: "100%" });
export const colorNote = style([quiet, { textAlign: "center" }]);

/** Card type: a field-like trigger holding the chosen types, opening a checklist. */
export const typeTrigger = style({
  appearance: "none",
  display: "flex",
  alignItems: "center",
  gap: "10px",
  minHeight: "50px",
  padding: "6px 16px 6px 10px",
  border: `1px solid ${t.color.formEdge}`,
  borderRadius: "10px",
  background: "#0b1016",
  color: shellTokens.ink,
  font: `22px/1 ${shellTokens.serif}`,
  textAlign: "left",
  cursor: "pointer",
  selectors: { "&:focus-visible": focus },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const typeChosen = style({
  flex: 1,
  display: "flex",
  flexWrap: "wrap",
  gap: "8px",
  minWidth: 0,
});
export const typePill = style({
  padding: "5px 12px",
  borderRadius: "8px",
  border: "1px solid #ffffff33",
  background: "#ffffff12",
  font: `20px/1 ${shellTokens.serif}`,
});
export const typeAny = style({ paddingLeft: "6px", color: "#cfc4b1" });
export const typeList = style({
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  gap: "8px 14px",
  padding: "12px 14px",
  border: `1px solid ${t.color.formEdge}`,
  borderRadius: "10px",
  background: "#0b1016",
});
export const rarities = style({
  display: "flex",
  flexWrap: "wrap",
  gap: "10px 18px",
});
export const range = style({
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  gap: "16px",
});
export const rangeError = style({
  margin: 0,
  font: `italic 21px/1.3 ${shellTokens.serif}`,
  color: "#ff9d7a",
});

/** A toast's place, above the dock on phones. */
export const notice = style({
  position: "absolute",
  zIndex: 6,
  right: "24px",
  bottom: "24px",
  selectors: { [phone]: { right: "16px", left: "16px", bottom: "120px" } },
});

/**
 * The chosen card's flight into the detail pane (see card-flight.ts): it
 * travels on the language's repositioning curve for one scene beat, while
 * everything else changes at once beneath it.
 */
const flight = ':root[data-flight="card"]';
globalStyle(`${flight}::view-transition-group(card-in-hand)`, {
  animationDuration: `${m.duration.scene}s`,
  animationTimingFunction: cssEase(m.ease.travel),
});
globalStyle(
  `${flight}::view-transition-old(card-in-hand), ${flight}::view-transition-new(card-in-hand)`,
  { height: "100%", animationDuration: `${m.duration.settle}s` },
);
globalStyle(
  `${flight}::view-transition-old(root), ${flight}::view-transition-new(root)`,
  { animationDuration: `${m.duration.settle}s` },
);
