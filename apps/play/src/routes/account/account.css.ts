import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/**
 * Account settings, as approved (AS-R01 to AS-R11). Lengths are in the pixel
 * scale of the approved references. A dialog narrower than `STACKED` puts the
 * sections above the content instead of beside it.
 */
const STACKED = "settings (inline-size < 46rem)";
/** Narrower than this, a purchase is drawn as two lines instead of a table row. */
const ROWS = "settings (inline-size < 36rem)";
const arrive = cssEase(m.ease.arrive);
const rule = "#a698774c";
const sans = '"Albert Sans", system-ui, sans-serif';
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;
const danger = "#e2705f";

/** The body of the main dialog: sections, a rule, and the chosen section. */
export const layout = style({
  containerName: "settings",
  containerType: "inline-size",
  position: "relative",
  minHeight: "524px",
  display: "flex",
});
export const columns = style({
  flex: 1,
  minWidth: 0,
  display: "grid",
  gridTemplateColumns: "284px 1px minmax(0, 1fr)",
  "@container": {
    [STACKED]: {
      gridTemplateColumns: "minmax(0, 1fr)",
      gridTemplateRows: "auto 1px minmax(0, 1fr)",
    },
  },
});
export const divider = style({ background: rule });

export const sections = style({
  position: "relative",
  display: "flex",
  flexDirection: "column",
  gap: "11px",
  padding: "10px 18px 10px 0",
  "@container": {
    // All four stay in view: a section scrolled off the edge is one nobody finds.
    [STACKED]: {
      flexDirection: "row",
      flexWrap: "wrap",
      gap: "6px",
      padding: "0 0 14px",
    },
  },
});
export const section = style({
  appearance: "none",
  position: "relative",
  flex: "none",
  minHeight: "57px",
  padding: "0 30px",
  border: "1px solid transparent",
  borderRadius: "8px",
  background: "transparent",
  color: shellTokens.ink,
  font: `25px/1.2 ${shellTokens.serif}`,
  textAlign: "start",
  whiteSpace: "nowrap",
  cursor: "pointer",
  selectors: {
    "&:hover:not([aria-disabled=true])": { background: "#ffffff0a" },
    "&:focus-visible": focus,
    '&[aria-selected="true"]': { color: shellTokens.active },
    '&[aria-disabled="true"]': { opacity: 0.45, cursor: "default" },
  },
  "@media": { [touch]: { minHeight: `max(57px, ${TOUCH_TARGET})` } },
  "@container": { [STACKED]: { minHeight: "48px", padding: "0 18px" } },
});
/** The filled plate that travels to the selected section. */
export const sectionPlate = style({
  position: "absolute",
  inset: -1,
  zIndex: 0,
  border: "1px solid #b9a67d",
  borderRadius: "8px",
  background: "linear-gradient(100deg,#37322a,#2a2722)",
  boxShadow: "inset 0 1px 0 #ffffff12",
});
export const sectionLabel = style({ position: "relative", zIndex: 1 });

export const panel = style({
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  padding: "8px 0 0 50px",
  "@container": { [STACKED]: { padding: "22px 0 0" } },
});
export const heading = style({
  margin: 0,
  font: `40px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  "@container": { [STACKED]: { fontSize: "32px" } },
});
export const headingRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  flexWrap: "wrap",
  gap: "14px 24px",
  padding: "10px 0 24px",
  borderBottom: `1px solid ${rule}`,
});
export const text = style({
  margin: 0,
  font: `25px/1.35 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
/** A lettered action of the settings: a little wider and taller than a toolbar button. */
export const action = style({
  selectors: { "&&": { minHeight: "55px", padding: "0 34px" } },
  "@media": {
    [touch]: {
      selectors: { "&&": { minHeight: `max(55px, ${TOUCH_TARGET})` } },
    },
  },
});
export const dangerAction = style({
  selectors: {
    "&&": {
      minHeight: "55px",
      padding: "0 28px",
      borderColor: danger,
      color: "#f08a78",
      background: "#2a100d66",
    },
    "&&:hover:not(:disabled)": { background: "#3a1511a6" },
  },
});
/** The confirming Delete account: filled, so it cannot be mistaken for Cancel. */
export const dangerConfirm = style({
  selectors: {
    "&&": {
      borderColor: "#e9a08f",
      color: "#fff3ea",
      background: "linear-gradient(#93261c,#7a1d15)",
    },
    "&&:hover:not(:disabled)": {
      background: "linear-gradient(#a52c21,#88211a)",
    },
  },
});

/* Profile */
export const person = style({
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  gap: "24px 34px",
  padding: "96px 0 0 36px",
  "@container": { [STACKED]: { padding: "18px 0 0", gap: "20px 24px" } },
});
export const personCopy = style({ minWidth: 0, flex: "1 1 280px" });
export const username = style({
  margin: 0,
  font: `44px/1.15 ${shellTokens.serif}`,
  fontWeight: 600,
  color: shellTokens.ink,
  overflowWrap: "anywhere",
  "@container": { [STACKED]: { fontSize: "34px" } },
});
export const personActions = style({
  display: "flex",
  flexWrap: "wrap",
  gap: "14px 22px",
  marginTop: "22px",
});

/* Patreon */
export const provider = style({
  display: "flex",
  alignItems: "center",
  gap: "22px",
  paddingTop: "40px",
  "@container": { [STACKED]: { paddingTop: "12px" } },
});
export const providerMark = style({
  flex: "none",
  width: "62px",
  height: "62px",
  color: "#fbeed6",
});
export const linkState = style({
  margin: "22px 0 0",
  font: `30px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const linkDetail = style({
  display: "flex",
  alignItems: "center",
  gap: "20px",
  margin: "18px 0 0",
  font: `28px/1.2 ${shellTokens.serif}`,
});
export const quiet = style({
  margin: "16px 0 0",
  font: `25px/1.35 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const providerAction = style({ marginTop: "30px" });

/* Billing */
export const subheading = style({
  margin: "26px 0 14px",
  font: `33px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
});
export const ledger = style({
  position: "relative",
  flex: "0 1 auto",
  maxHeight: "278px",
  overflowY: "auto",
  border: "1px solid #8d8168",
  borderRadius: "10px",
  background: "#0a1016",
  scrollbarWidth: "thin",
  scrollbarColor: "#8d8168 transparent",
  selectors: { "&:focus-visible": focus },
});
export const table = style({
  width: "100%",
  borderCollapse: "separate",
  borderSpacing: 0,
  font: `23px/1.2 ${shellTokens.serif}`,
  "@container": { [STACKED]: { fontSize: "20px" } },
});
globalStyle(`${table} th, ${table} td`, {
  padding: "0 26px",
  height: "55px",
  textAlign: "start",
  whiteSpace: "nowrap",
  "@container": { [STACKED]: { paddingInline: "14px" } },
});
globalStyle(`${table} th`, {
  position: "sticky",
  top: 0,
  zIndex: 1,
  height: "52px",
  fontWeight: 400,
  color: shellTokens.ink,
  background: "#141a20",
  borderBottom: "1px solid #8d816880",
});
globalStyle(`${table} tbody tr + tr td`, {
  borderTop: `1px solid ${rule}`,
});
globalStyle(`${table} td:first-child`, {
  whiteSpace: "normal",
  overflowWrap: "anywhere",
});
// A phone has no room for four columns. Each purchase becomes two lines there:
// the item and its amount, then the date and status. The headings stay for
// assistive technology and are not drawn.
globalStyle(`${table}, ${table} tbody`, {
  "@container": { [ROWS]: { display: "block" } },
});
globalStyle(`${table} thead`, {
  "@container": {
    [ROWS]: {
      position: "absolute",
      width: "1px",
      height: "1px",
      overflow: "hidden",
      clipPath: "inset(50%)",
    },
  },
});
globalStyle(`${table} tbody tr`, {
  "@container": {
    [ROWS]: {
      display: "grid",
      gridTemplateColumns: "minmax(0, 1fr) auto",
      gridTemplateAreas: '"item amount" "date status"',
      gap: "4px 16px",
      padding: "14px 18px",
    },
  },
});
globalStyle(`${table} tbody tr + tr`, {
  "@container": { [ROWS]: { borderTop: `1px solid ${rule}` } },
});
// The wide table draws its row rule on each cell. Here the row draws it once.
globalStyle(`${table} tbody tr + tr td`, {
  "@container": { [ROWS]: { borderTop: 0 } },
});
globalStyle(`${table} tbody td`, {
  "@container": {
    [ROWS]: { display: "block", height: "auto", padding: 0, border: 0 },
  },
});
globalStyle(`${table} tbody td:nth-child(1)`, {
  "@container": { [ROWS]: { gridArea: "item", minWidth: 0 } },
});
globalStyle(`${table} tbody td:nth-child(2)`, {
  "@container": { [ROWS]: { gridArea: "date", color: "#d8cdb9" } },
});
globalStyle(`${table} tbody td:nth-child(3)`, {
  "@container": { [ROWS]: { gridArea: "amount", justifySelf: "end" } },
});
globalStyle(`${table} tbody td:nth-child(4)`, {
  "@container": {
    [ROWS]: { gridArea: "status", justifySelf: "end", color: "#d8cdb9" },
  },
});
/** Where the next page of purchases is asked for as it scrolls into view. */
export const more = style({ height: "1px" });
export const ledgerNote = style({
  display: "grid",
  placeItems: "center",
  minHeight: "164px",
  font: `italic 25px/1.3 ${shellTokens.serif}`,
  color: "#d8cdb9",
});
export const skeletonRows = style({
  display: "grid",
  gap: "22px",
  padding: "22px 26px",
});

/* Account */
export const settingRow = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  flexWrap: "wrap",
  gap: "14px 24px",
  paddingTop: "28px",
});
export const requested = style({ paddingTop: "12px" });
export const footer = style({
  marginTop: "auto",
  display: "flex",
  justifyContent: "flex-end",
  paddingTop: "24px",
});

/** One line of passive feedback, held in the dialog's lower corner. */
export const notice = style({
  position: "absolute",
  right: 0,
  bottom: 0,
  width: "min(100%, 520px)",
  pointerEvents: "none",
});
/** The picture's Save, filled as approved: the one committing action of its stage. */
export const goldAction = style({
  selectors: {
    "&&": {
      borderColor: "#fff3d6",
      color: "#1c1508",
      background: "linear-gradient(#f6e2b4,#dfbd77)",
      fontWeight: 600,
    },
    "&&:hover:not(:disabled)": {
      background: "linear-gradient(#fbebc6,#e7c884)",
    },
    "&&:disabled": { opacity: 0.6 },
  },
});
/**
 * Feedback in a child dialog. It sits beside the actions and takes no height
 * of its own, so the dialog keeps its approved size.
 */
export const childNotice = style({
  position: "absolute",
  left: "32px",
  bottom: "26px",
  width: "min(calc(100% - 64px), 330px)",
  pointerEvents: "none",
});

/* Profile picture: one shell for choosing, cropping and no results. */
export const pictureShell = style({
  selectors: {
    "&&": {
      width: "757px",
      height: "704px",
      // On a phone the designed height is too short for touch-sized controls,
      // so the shell takes more of the screen, and never more than the screen.
      minHeight: "min(620PX, calc(100dvh - 24PX))",
    },
    // As a drawer the editor takes the height the screen allows.
    '&&[data-presentation="drawer"]': {
      height: "calc(100dvh - 28PX)",
      minHeight: 0,
    },
  },
});
export const pictureBody = style({
  display: "flex",
  flexDirection: "column",
  minHeight: 0,
  selectors: {
    // Choosing scrolls its own grid. Cropping never squeezes its controls into
    // each other: on a very short screen the dialog scrolls instead.
    '&[data-picture-stage="crop"]': { minHeight: "296PX" },
  },
});
export const artGrid = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  display: "grid",
  gridTemplateColumns: "repeat(auto-fill, minmax(min(100%, 240px), 1fr))",
  alignContent: "start",
  gap: "18px 20px",
  margin: "20px -6px 0",
  padding: "6px",
  scrollbarWidth: "thin",
  scrollbarColor: "#8d8168 transparent",
});
export const artTile = style({
  appearance: "none",
  display: "grid",
  gap: "8px",
  padding: 0,
  border: 0,
  background: "transparent",
  color: shellTokens.ink,
  font: `italic 20px/1.2 ${sans}`,
  textAlign: "center",
  cursor: "pointer",
  selectors: { "&:focus-visible": { outline: "none" } },
});
export const artFrame = style({
  display: "block",
  aspectRatio: "305 / 180",
  overflow: "hidden",
  borderRadius: "3px",
  outline: "2px solid transparent",
  outlineOffset: "1px",
  background: "#10161c",
  selectors: {
    [`${artTile}:hover &`]: { outlineColor: "#f2deb080" },
    [`${artTile}:focus-visible &`]: { outlineColor: "#f2deb0" },
  },
  "@media": {
    [standardMotion]: {
      transition: `outline-color ${m.duration.feedback}s ${arrive}`,
    },
  },
});
globalStyle(`${artFrame} > img`, {
  display: "block",
  width: "100%",
  height: "100%",
  objectFit: "cover",
});
globalStyle(`${artTile}:hover ${artFrame} > img`, {
  "@media": {
    [standardMotion]: {
      transform: "scale(1.04)",
      transition: `transform ${m.duration.scene}s ${arrive}`,
    },
  },
});
export const noArt = style({
  flex: 1,
  display: "grid",
  placeContent: "center",
  gap: "10px",
  textAlign: "center",
});
export const noArtTitle = style({
  margin: 0,
  font: `36px/1.15 ${shellTokens.serif}`,
  fontWeight: 600,
});
export const cropHint = style({
  margin: "0 0 10px",
  font: `italic 20px/1.2 ${sans}`,
  textAlign: "center",
});
export const cropStage = style({
  position: "relative",
  alignSelf: "center",
  flex: "0 1 380px",
  aspectRatio: "1",
  minHeight: "160PX",
  maxWidth: "100%",
  overflow: "hidden",
  border: "1px solid #f4ead6",
  background: "#05080c",
  cursor: "grab",
  touchAction: "none",
  selectors: {
    "&:active": { cursor: "grabbing" },
    "&:focus-visible": focus,
  },
});
/** Everything outside the circle, dimmed. The picture itself is never altered. */
export const cropShade = style({
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
  background:
    "radial-gradient(circle closest-side, transparent calc(100% - 1.5px), #05080c99 100%)",
});
export const cropGuide = style({
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
  borderRadius: "50%",
  border: "1.5px solid #fbf3e2",
  // Faint thirds, to help centre a face.
  backgroundImage:
    "linear-gradient(to right, transparent calc(33.33% - 0.5px), #ffffff2e calc(33.33% - 0.5px), #ffffff2e calc(33.33% + 0.5px), transparent calc(33.33% + 0.5px), transparent calc(66.66% - 0.5px), #ffffff2e calc(66.66% - 0.5px), #ffffff2e calc(66.66% + 0.5px), transparent calc(66.66% + 0.5px)), linear-gradient(to bottom, transparent calc(33.33% - 0.5px), #ffffff2e calc(33.33% - 0.5px), #ffffff2e calc(33.33% + 0.5px), transparent calc(33.33% + 0.5px), transparent calc(66.66% - 0.5px), #ffffff2e calc(66.66% - 0.5px), #ffffff2e calc(66.66% + 0.5px), transparent calc(66.66% + 0.5px))",
});
export const cropCredit = style({
  margin: "10px 0 10px",
  font: `italic 20px/1.2 ${sans}`,
  textAlign: "center",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const zoom = style({
  alignSelf: "center",
  width: "min(100%, 590px)",
});

/* Badge */
export const badgeBody = style({
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  gap: "20px 34px",
  padding: "6px 6px 4px 26px",
});
export const badgeRule = style({
  alignSelf: "stretch",
  width: "1px",
  background: rule,
});
export const badgeMark = style({
  display: "inline-block",
  width: "0.8em",
  height: "0.8em",
  verticalAlign: "-0.06em",
  color: "#ffe8b9",
});
// On a phone the chooser shows two artworks across, so more are in view at once.
globalStyle(`${pictureShell}[data-presentation="drawer"] ${artGrid}`, {
  gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
  gap: "16px 14px",
});
globalStyle(`${pictureShell}[data-presentation="drawer"] ${artTile}`, {
  fontSize: "18px",
});
// A drawer is as wide as the phone, so the crop stage takes that width, or
// about half the screen's height on a phone held sideways. It stays square:
// the crop is worked out against a square.
globalStyle(`${pictureShell}[data-presentation="drawer"] ${cropStage}`, {
  flex: "0 0 auto",
  width: "min(100%, 52dvh)",
});
// The picture shell's body is a column, so its stages can fill the fixed height.
globalStyle(`${pictureShell} > [data-dialog-body]`, {
  display: "flex",
  flexDirection: "column",
});
globalStyle(`${pictureBody}`, { flex: 1 });
