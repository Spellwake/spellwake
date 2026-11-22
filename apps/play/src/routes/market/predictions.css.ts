import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * Market Predictions, as approved (EX, PM, PC, BF and RS references): the
 * title row with search and Create prediction, Explore and My predictions
 * tabs over one scrolling body, question cards, personal bet cards and
 * authored rows, and the bet, result and creation dialogs. Lengths are in the
 * approved references' pixel scale. Card grids narrow by their own width.
 */
const page = "predictions";
const TWO = `${page} (inline-size < 70rem)`;
const ONE = `${page} (inline-size < 44rem)`;
/** Where the title, search and Create no longer fit on one row. */
const HEAD_STACK = `${page} (inline-size < 60rem)`;
const phone = ':root[data-device="phone"] &';
const compact = ':root[data-shell="compact"] &';
const champagne = "#e7cf9f";
const gold = "#d9b878";
const edge = "#cdb48a";
const quietEdge = "#8f8064";
const rule = "#a698774c";
const muted = "#b9ae99";
const danger = "#e98a7a";
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;
const panel = "linear-gradient(180deg,#0d1218e6,#0a0e13f0)";

export const root = style({
  containerName: page,
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  padding: "26px 3% 0 32px",
  selectors: { [phone]: { padding: "14px 16px 0" } },
});

/** Title, search and Create prediction; the tabs sit under them. */
export const head = style({
  flexShrink: 0,
  display: "grid",
  gridTemplateColumns: "max-content minmax(0,360px) max-content",
  justifyContent: "space-between",
  alignItems: "center",
  gap: "14px 24px",
  "@container": {
    [HEAD_STACK]: {
      gridTemplateColumns: "minmax(0,1fr) max-content",
      gap: "12px",
    },
  },
});
export const title = style({
  position: "relative",
  justifySelf: "start",
  margin: 0,
  font: `52px/1.1 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  whiteSpace: "nowrap",
  "@container": { [ONE]: { fontSize: "38px" } },
});
/** A drawn trail of sparks after the title, as the approved header shows. */
export const trail = style({
  position: "absolute",
  left: "calc(100% + 6px)",
  top: "18%",
  width: "190px",
  height: "44px",
  pointerEvents: "none",
  "@container": { [ONE]: { display: "none" } },
});
export const searchCell = style({
  minWidth: 0,
  "@container": { [HEAD_STACK]: { gridColumn: "1 / -1", gridRow: 2 } },
});
export const createButton = style({
  selectors: { "&&": { whiteSpace: "nowrap" } },
});
globalStyle(`${createButton} svg`, {
  width: "1.1em",
  height: "1.1em",
  marginRight: "8px",
  verticalAlign: "-0.15em",
});

export const tabs = style({
  flexShrink: 0,
  display: "flex",
  gap: "8px",
  marginTop: "10px",
  borderBottom: `1px solid ${rule}`,
});
export const tab = style({
  position: "relative",
  minHeight: "44PX",
  padding: "10px 24px 14px",
  border: 0,
  background: "none",
  font: `24px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  cursor: "pointer",
  transition: `color 180ms ease-out`,
  selectors: {
    '&[aria-selected="true"]': { color: champagne },
    "&:hover": { color: champagne },
    "&:focus-visible": focus,
  },
  "@container": { [ONE]: { fontSize: "21px", padding: "8px 14px 12px" } },
});
/** The selected tab's underline and its small diamond. */
export const tabMark = style({
  position: "absolute",
  left: 0,
  right: 0,
  bottom: "-1px",
  height: "2px",
  background: `linear-gradient(90deg,transparent,${gold} 18%,${gold} 82%,transparent)`,
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      left: "50%",
      top: "-3px",
      width: "8px",
      height: "8px",
      background: gold,
      transform: "translateX(-50%) rotate(45deg)",
    },
  },
});

/** The one scrolling body under the fixed header. */
export const body = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  padding: "26px 2px 48px",
  selectors: {
    [compact]: { paddingBottom: "130px" },
    [phone]: { paddingBottom: "150px" },
  },
});

export const board = style({
  listStyle: "none",
  margin: 0,
  padding: 0,
  display: "grid",
  gridTemplateColumns: "repeat(3, minmax(0,1fr))",
  gap: "24px",
  "@container": {
    [TWO]: { gridTemplateColumns: "repeat(2, minmax(0,1fr))" },
    [ONE]: { gridTemplateColumns: "minmax(0,1fr)", gap: "16px" },
  },
});

/** A question card. Its action's press area covers the whole card. */
export const card = style({
  position: "relative",
  display: "flex",
  flexDirection: "column",
  minWidth: 0,
  padding: "24px 28px 20px",
  border: `1.5px solid ${quietEdge}80`,
  borderRadius: "14px",
  background: panel,
  boxShadow: "0 16px 40px #0007",
  transition: `border-color 180ms ease-out, box-shadow 180ms ease-out`,
  selectors: {
    "&:hover, &:focus-within": {
      borderColor: edge,
      boxShadow: `0 16px 40px #0007, 0 0 0 1px ${edge}33, 0 0 28px ${gold}26`,
    },
  },
  "@container": { [ONE]: { padding: "18px 18px 14px" } },
});
export const identity = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr)",
  alignItems: "center",
  gap: "20px",
});
export const art = style({
  position: "relative",
  display: "block",
  overflow: "hidden",
  flexShrink: 0,
  width: "100px",
  aspectRatio: "1",
  borderRadius: "6px",
  border: `1px solid ${edge}66`,
  background: "radial-gradient(circle at 50% 35%,#3a3020,#14110c 70%)",
  boxShadow: "0 6px 18px #0009",
  "@container": { [ONE]: { width: "76px" } },
});
export const artSmall = style({ width: "72px" });
/** The printed card, scaled so its artwork alone fills the square. */
export const artImage = style({
  position: "absolute",
  inset: 0,
  backgroundRepeat: "no-repeat",
  backgroundSize: "180% auto",
  backgroundPosition: "50% 20%",
});
export const artGlyph = style({
  position: "absolute",
  inset: 0,
  margin: "auto",
  width: "40%",
  height: "40%",
  color: `${champagne}99`,
});
export const name = style({
  margin: 0,
  font: `25px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const meta = style({
  margin: "6px 0 0",
  font: `19px/1.3 ${shellTokens.serif}`,
  color: muted,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const question = style({
  margin: "22px 0 18px",
  font: `31px/1.25 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": { [ONE]: { fontSize: "25px", margin: "16px 0 14px" } },
});
export const dateChip = style({
  display: "inline-flex",
  alignItems: "center",
  alignSelf: "flex-start",
  gap: "10px",
  padding: "6px 16px",
  borderRadius: "999px",
  background: "#ffffff0d",
  border: "1px solid #ffffff12",
  font: `19px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
  whiteSpace: "nowrap",
});
globalStyle(`${dateChip} svg`, {
  width: "1em",
  height: "1em",
  color: muted,
});
export const cardFoot = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "12px",
  marginTop: "auto",
  paddingTop: "16px",
  borderTop: `1px solid ${rule}`,
});
export const cardSpacer = style({ flex: 1, minHeight: "18px" });
export const statusRow = style({ marginTop: "12px" });
/** A text action whose press area stretches over its card. */
export const cardAction = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "10px",
  minHeight: "44PX",
  padding: 0,
  border: 0,
  background: "none",
  font: `23px/1.2 ${shellTokens.serif}`,
  color: champagne,
  cursor: "pointer",
  selectors: {
    "&::after": { content: '""', position: "absolute", inset: 0 },
    "&:focus-visible": { outline: "none" },
    "&:focus-visible::after": {
      ...focus,
      borderRadius: "14px",
    },
  },
});
globalStyle(`${cardAction} svg`, {
  width: "1em",
  height: "1em",
  transition: `transform 180ms ease-out`,
});
globalStyle(`${card}:hover ${cardAction} svg`, {
  "@media": { [standardMotion]: { transform: "translateX(4px)" } },
});

/** Yes, No, Void, Pending and lifecycle pills. */
export const pill = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "8px",
  padding: "4px 18px",
  borderRadius: "999px",
  border: "1px solid #ffffff1f",
  background: "#ffffff0f",
  font: `20px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
  whiteSpace: "nowrap",
  selectors: {
    '&[data-tone="yes"]': {
      background: "#1d2e1c",
      borderColor: "#4d6f40",
      color: "#d5e6bd",
    },
    '&[data-tone="no"]': {
      background: "#3a1716",
      borderColor: "#8a3a32",
      color: "#f2b7ab",
    },
    '&[data-tone="pending"]': {
      background: "#2a2418",
      borderColor: "#5f5239",
      color: champagne,
    },
    '&[data-tone="open"]': {
      background: "#1b2a1b",
      borderColor: "#4d6f40",
      color: "#d5e6bd",
    },
  },
});
globalStyle(`${pill} svg`, { width: "0.9em", height: "0.9em" });
export const status = style({
  font: `20px/1.3 ${shellTokens.serif}`,
  color: champagne,
  whiteSpace: "nowrap",
  selectors: { '&[data-tone="lost"]': { color: danger } },
});

/** My predictions. */
export const groupTitle = style({
  margin: "0 0 14px",
  font: `33px/1.2 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  selectors: { "& ~ &": { marginTop: "34px" } },
});
export const group = style({ marginBottom: "36px" });
export const bets = style({
  listStyle: "none",
  margin: 0,
  padding: 0,
  display: "grid",
  gridTemplateColumns: "repeat(2, minmax(0,1fr))",
  gap: "22px",
  "@container": { [TWO]: { gridTemplateColumns: "minmax(0,1fr)" } },
});
export const betCard = style([card, { padding: "20px 24px 14px" }]);
export const betHead = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr) auto",
  alignItems: "center",
  gap: "18px",
});
export const betTags = style({
  display: "flex",
  alignItems: "center",
  gap: "14px",
  alignSelf: "start",
  "@container": { [ONE]: { gridColumn: "1 / -1", gridRow: 2 } },
});
globalStyle(`${betTags} > * + *`, {
  paddingLeft: "14px",
  borderLeft: `1px solid ${rule}`,
});
export const betQuestion = style({
  margin: "14px 0 12px",
  font: `25px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const amounts = style({
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  gap: "10px 18px",
  font: `21px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const amountLabel = style({ color: muted, marginRight: "10px" });
globalStyle(`${amounts} > svg`, { width: "1.1em", height: "1.1em" });
export const rowAction = style([
  cardAction,
  {
    marginLeft: "auto",
    paddingLeft: "18px",
    borderLeft: `1px solid ${rule}`,
    fontSize: "21px",
  },
]);
export const createdRows = style({
  listStyle: "none",
  margin: 0,
  padding: 0,
  display: "grid",
  gap: "14px",
});
export const createdRow = style([
  card,
  {
    display: "grid",
    gridTemplateColumns:
      "minmax(0,250px) minmax(0,1fr) max-content max-content",
    alignItems: "center",
    gap: "20px",
    padding: "14px 24px",
    "@container": {
      [ONE]: {
        gridTemplateColumns: "minmax(0,1fr) max-content",
        padding: "14px 16px",
      },
    },
  },
]);
export const createdQuestion = style({
  minWidth: 0,
  paddingLeft: "20px",
  borderLeft: `1px solid ${rule}`,
  "@container": {
    [ONE]: { gridColumn: "1 / -1", paddingLeft: 0, borderLeft: 0 },
  },
});
globalStyle(`${createdQuestion} > p`, {
  margin: "0 0 8px",
  font: `23px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});

/** Empty, no-match, read and loading states. */
export const quiet = style({
  padding: "30px 24px",
  border: `1px solid ${quietEdge}55`,
  borderRadius: "12px",
  background: "#0a0e13a6",
  textAlign: "center",
  font: `italic 24px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const quietRow = style([
  quiet,
  {
    display: "flex",
    alignItems: "center",
    gap: "18px",
    padding: "14px 24px",
    textAlign: "left",
    fontSize: "21px",
  },
]);
globalStyle(`${quietRow} svg`, { width: "26px", height: "26px", color: muted });
export const emptyPanel = style([
  quiet,
  { padding: "46px 24px", fontStyle: "normal" },
]);
export const emptyTitle = style({
  margin: "0 0 14px",
  font: `31px/1.2 ${shellTokens.serif}`,
});
export const emptyAction = style([
  cardAction,
  { selectors: { "&::after": { content: "none" } } },
]);
export const accountGap = style([emptyPanel, { marginTop: "20px" }]);
const shimmer = keyframes({
  "0%": { backgroundPosition: "100% 0" },
  "100%": { backgroundPosition: "-100% 0" },
});
export const skeleton = style({
  display: "block",
  borderRadius: "6px",
  background:
    "linear-gradient(90deg,#ffffff0d 30%,#ffffff1f 50%,#ffffff0d 70%)",
  backgroundSize: "200% 100%",
  "@media": {
    [standardMotion]: { animation: `${shimmer} 1.6s linear infinite` },
  },
});
const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  display: "inline-block",
  width: "28px",
  height: "28px",
  borderRadius: "50%",
  border: `2px solid ${gold}44`,
  borderTopColor: gold,
  "@media": { [standardMotion]: { animation: `${spin} 0.9s linear infinite` } },
});
export const appending = style({
  display: "flex",
  justifyContent: "center",
  padding: "24px 0 8px",
});
export const srOnly = style({
  position: "absolute",
  width: "1px",
  height: "1px",
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
});

/** Dialog bodies. */
export const dialogItem = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr)",
  alignItems: "center",
  gap: "22px",
  paddingBottom: "18px",
});
export const dialogQuestion = style({
  margin: 0,
  font: `27px/1.25 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const dialogMeta = style({
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  gap: "6px 12px",
  margin: "8px 0 0",
  font: `18px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const sideChip = style([
  pill,
  { padding: "1px 12px", fontSize: "17px" },
]);
export const section = style({
  padding: "16px 4px",
  borderTop: `1px solid ${rule}`,
});
export const resultRow = style([
  section,
  { display: "flex", alignItems: "center", gap: "28px" },
]);
export const resultLabel = style({
  font: `22px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const exchange = style([
  section,
  {
    display: "grid",
    gridTemplateColumns: "minmax(0,1fr) auto minmax(0,1fr)",
    alignItems: "end",
    gap: "16px",
  },
]);
export const exchangeLabel = style({
  display: "block",
  marginBottom: "8px",
  font: `21px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const exchangeValue = style({
  font: `32px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const exchangeArrow = style({
  width: "28px",
  height: "28px",
  color: champagne,
  marginBottom: "10px",
});
export const verdict = style([
  section,
  { textAlign: "center", paddingBottom: "6px" },
]);
export const verdictTitle = style({
  margin: 0,
  font: `38px/1.15 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const verdictNote = style({
  margin: "6px 0 0",
  font: `20px/1.3 ${shellTokens.serif}`,
  color: champagne,
  selectors: { '&[data-tone="quiet"]': { color: muted, fontStyle: "italic" } },
});
export const publicNote = style([
  section,
  {
    textAlign: "center",
    font: `21px/1.3 ${shellTokens.serif}`,
    color: shellTokens.ink,
  },
]);
export const blockNote = style({
  margin: "8px 0 6px",
  font: `22px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});

/** The bet form: amount, arrow, winnings, then the two sides. */
export const betRow = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,1.15fr) auto minmax(0,1fr)",
  alignItems: "end",
  gap: "16px 22px",
  paddingTop: "4px",
});
export const coinField = style({
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
  display: "flex",
  alignItems: "center",
  gap: "12px",
  height: "58px",
  padding: "0 16px",
  border: `1.5px solid ${edge}`,
  borderRadius: "8px",
  background: "#05080bcc",
  selectors: {
    "&:focus-within": focus,
    '&[data-invalid="true"]': { borderColor: danger },
  },
});
export const coinIcon = style({ width: "30px", height: "30px", flexShrink: 0 });
export const coinInput = style({
  alignSelf: "stretch",
  flex: 1,
  minWidth: 0,
  border: 0,
  background: "none",
  font: `32px/1 ${shellTokens.serif}`,
  color: shellTokens.ink,
  outline: "none",
  selectors: { "&:disabled": { opacity: 0.7 } },
});
export const winnings = style({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  minHeight: "58px",
  font: `32px/1 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const winningsQuiet = style({
  font: `italic 22px/1.2 ${shellTokens.serif}`,
  color: champagne,
});
export const winningsOff = style({
  font: `27px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const fieldError = style({
  margin: "8px 0 0",
  font: `19px/1.3 ${shellTokens.serif}`,
  color: danger,
});
export const fieldNote = style({
  margin: "10px 0 0",
  textAlign: "center",
  font: `italic 20px/1.3 ${shellTokens.serif}`,
  color: champagne,
});
export const sides = style({
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  gap: "18px",
  marginTop: "20px",
});
export const sideButton = style({
  selectors: {
    "&&": { height: "56px", fontSize: "25px" },
    '&&[data-armed="true"]': {
      background: "linear-gradient(180deg,#6f6243,#4b412b)",
      borderColor: edge,
      color: "#fff6e2",
    },
  },
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
});
export const smallSpinner = style([spinner, { width: "18px", height: "18px" }]);

/** Create prediction. */
export const createBody = style({
  display: "grid",
  gap: "16px",
});
export const chosen = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr) auto",
  alignItems: "center",
  gap: "20px",
});
export const placeholderArt = style({
  display: "grid",
  placeItems: "center",
  width: "72px",
  aspectRatio: "1",
  borderRadius: "6px",
  border: `1px dashed ${quietEdge}`,
  color: muted,
});
globalStyle(`${placeholderArt} svg`, { width: "28px", height: "28px" });
export const smallButton = style({
  selectors: { "&&": { height: "40px", fontSize: "19px", padding: "0 18px" } },
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
});
export const pair = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
  gap: "18px",
  "@container": { [ONE]: { gridTemplateColumns: "minmax(0,1fr)" } },
});
export const fieldLabel = style({
  display: "block",
  marginBottom: "8px",
  font: `20px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const inputBox = style({
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
  position: "relative",
  display: "flex",
  alignItems: "center",
  gap: "10px",
  height: "52px",
  padding: "0 14px",
  border: `1.5px solid ${quietEdge}`,
  borderRadius: "8px",
  background: "#05080bcc",
  font: `24px/1 ${shellTokens.serif}`,
  color: shellTokens.ink,
  selectors: {
    "&:focus-within": { borderColor: edge, ...focus },
    '&[data-invalid="true"]': { borderColor: danger },
  },
});
export const dateButton = style([
  inputBox,
  {
    width: "100%",
    cursor: "pointer",
    textAlign: "left",
    selectors: {
      "&:focus-visible": { borderColor: edge, ...focus },
      '&[aria-expanded="true"]': { borderColor: edge },
    },
  },
]);
globalStyle(`${dateButton} svg`, {
  width: "22px",
  height: "22px",
  color: muted,
});
export const plainInput = style({
  alignSelf: "stretch",
  display: "flex",
  alignItems: "center",
  flex: 1,
  minWidth: 0,
  border: 0,
  background: "none",
  font: "inherit",
  color: "inherit",
  outline: "none",
});
export const closesNote = style({
  margin: 0,
  font: `19px/1.3 ${shellTokens.serif}`,
  color: muted,
});
export const checkingLine = style({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  margin: 0,
  font: `20px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const blockedLine = style({
  margin: "-6px 0 0",
  font: `19px/1.3 ${shellTokens.serif}`,
  color: danger,
});
export const wide = style({
  selectors: { "&&": { width: "100%", height: "54px", fontSize: "24px" } },
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
});

/** The end-date calendar. */
export const calendar = style({
  // Seven days of at least a full touch width each on a touch screen.
  "@media": { "(pointer: coarse)": { width: "min(100%, 372PX)" } },
  selectors: {
    [phone]: {
      justifySelf: "start",
      width: "min(100vw, 460PX)",
      marginLeft: "calc(50% - min(50vw, 230PX))",
      paddingLeft: 0,
      paddingRight: 0,
      borderLeft: 0,
      borderRight: 0,
      borderRadius: 0,
    },
  },
  justifySelf: "end",
  width: "min(340px, 100%)",
  padding: "14px 14px 12px",
  border: `1.5px solid ${edge}`,
  borderRadius: "12px",
  background: "#0b0f14",
  boxShadow: "0 18px 46px #000c",
});
export const calendarHead = style({
  display: "grid",
  gridTemplateColumns: "auto 1fr auto",
  alignItems: "center",
  marginBottom: "8px",
  font: `22px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  textAlign: "center",
});
export const monthButton = style({
  "@media": {
    "(pointer: coarse)": {
      selectors: { "&&": { minWidth: "44PX", minHeight: "44PX" } },
    },
  },
  display: "grid",
  placeItems: "center",
  width: "40px",
  height: "40px",
  border: 0,
  borderRadius: "8px",
  background: "none",
  color: champagne,
  cursor: "pointer",
  selectors: {
    "&:hover:not(:disabled)": { background: "#ffffff12" },
    "&:focus-visible": focus,
    "&:disabled": { opacity: 0.35, cursor: "default" },
  },
});
export const weekdays = style({
  display: "grid",
  gridTemplateColumns: "repeat(7,1fr)",
  font: `16px/2 ${shellTokens.serif}`,
  color: muted,
  textAlign: "center",
});
export const days = style({
  display: "grid",
  gridTemplateColumns: "repeat(7,1fr)",
  gap: "2px",
  selectors: { [phone]: { gap: 0 } },
});
export const day = style({
  height: "38px",
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
  border: 0,
  borderRadius: "6px",
  background: "none",
  font: `18px/1 ${shellTokens.serif}`,
  color: shellTokens.ink,
  cursor: "pointer",
  selectors: {
    "&:hover:not(:disabled)": { background: "#ffffff14" },
    "&:focus-visible": focus,
    "&:disabled": { color: "#ffffff38", cursor: "default" },
    '&[aria-selected="true"]': {
      background: gold,
      color: "#17120a",
      fontWeight: 600,
    },
  },
});

/** The card picker. */
export const pickerHead = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr)",
  alignItems: "center",
  gap: "12px",
});
export const backButton = style([
  monthButton,
  { width: "44px", height: "44px" },
]);
export const results = style({
  listStyle: "none",
  margin: 0,
  padding: "2px",
  display: "grid",
  gap: "4px",
  maxHeight: "min(330px, 42dvh)",
  overflowY: "auto",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
});
export const result = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr) auto",
  alignItems: "center",
  gap: "18px",
  width: "100%",
  padding: "8px 14px",
  border: "1px solid transparent",
  borderRadius: "8px",
  background: "none",
  textAlign: "left",
  color: shellTokens.ink,
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#ffffff0d" },
    "&:focus-visible": focus,
    '&[aria-selected="true"]': {
      background: "#ffffff14",
      borderColor: `${edge}aa`,
    },
  },
});
globalStyle(`${result} > svg`, {
  width: "26px",
  height: "26px",
  color: champagne,
});
export const resultName = style({
  display: "block",
  font: `22px/1.2 ${shellTokens.serif}`,
});
export const resultMeta = style({
  display: "block",
  marginTop: "4px",
  font: `18px/1.2 ${shellTokens.serif}`,
  color: muted,
});
export const pickerNote = style({
  margin: "6px 0",
  textAlign: "center",
  font: `20px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});

/** Inline notices in the dialogs, at the size of their other quiet lines. */
export const notice = style({ fontSize: "13px", justifySelf: "stretch" });
