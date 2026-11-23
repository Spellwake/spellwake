import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/**
 * The friend-game lobby, as approved (lobby-contracts-approved-v3): players on
 * the left, your deck and the rules on the right, the actions below. Lengths
 * are in the pixel scale of the approved references (`host-filled.png`).
 * Narrower than `STACKED` the columns stack and each seat's controls wrap
 * under its name.
 */
const STACKED = "lobby (inline-size < 80rem)";
const NARROW = "lobby (inline-size < 44rem)";
const phone = ':root[data-device="phone"] &';
/** Only an upright phone keeps the actions pinned above the dock; on its side there is no room. */
const phonePortrait =
  ':root[data-device="phone"][data-orientation="portrait"] &';
const champagne = "#e7cf9f";
const panelEdge = "#cdb48a8c";
const rule = "#a698774c";
const arrive = cssEase(m.ease.arrive);
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;
const panel = {
  border: `1px solid ${panelEdge}`,
  borderRadius: "14px",
  background: "linear-gradient(180deg,#0d1218e6,#0a0e13f0)",
  boxShadow: "0 20px 50px #0008, inset 0 1px 0 #ffffff0d",
} as const;

export const page = style({
  containerName: "lobby",
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  padding: "48px 5% 40px 70px",
  selectors: { [phone]: { padding: "18px 16px 150px" } },
});
export const head = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr)",
  alignItems: "start",
  gap: "8px 40px",
  marginBottom: "40px",
  "@container": {
    [NARROW]: { gap: "14px", marginBottom: "22px" },
  },
});
export const back = style({ marginTop: "6px" });
export const title = style({
  margin: 0,
  font: `44px/1.1 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  selectors: { [phone]: { fontSize: "36px" } },
});
export const subtitle = style({
  margin: "10px 0 0",
  font: `24px/1.25 ${shellTokens.serif}`,
  color: champagne,
  selectors: { [phone]: { fontSize: "21px" } },
});

export const layout = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,936px) minmax(300px,475px)",
  alignItems: "start",
  gap: "28px",
  "@container": { [STACKED]: { gridTemplateColumns: "minmax(0,1fr)" } },
});
export const players = style({
  ...panel,
  padding: "18px 16px 18px",
});
export const playersHead = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "16px",
  padding: "2px 4px 18px 18px",
});
export const playersTitle = style({
  margin: 0,
  font: `34px/1.1 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
});
export const count = style({
  marginLeft: "22px",
  fontSize: "25px",
  color: champagne,
});
export const quotaLine = style({
  margin: "0 0 12px 18px",
});
export const seats = style({
  display: "flex",
  flexDirection: "column",
  gap: "12px",
  margin: 0,
  padding: 0,
  listStyle: "none",
});
/** One seat: picture and name, then its controls, then its readiness. */
export const seat = style({
  display: "grid",
  gridTemplateColumns: "76px minmax(150px,170px) 1px minmax(0,1fr) auto",
  alignItems: "center",
  gap: "0 20px",
  minHeight: "100px",
  padding: "10px 14px 10px 20px",
  border: "1px solid #ffffff12",
  borderRadius: "10px",
  background: "#ffffff06",
  "@container": {
    [NARROW]: {
      gridTemplateColumns: "max-content minmax(0,1fr) auto",
      gridTemplateAreas: '"pic name ready" "controls controls controls"',
      gap: "12px 14px",
      padding: "12px 14px",
    },
  },
});
export const pic = style({
  display: "grid",
  placeItems: "center",
  "@container": { [NARROW]: { gridArea: "pic" } },
});
export const aiMark = style({
  display: "grid",
  placeItems: "center",
  width: "76px",
  height: "76px",
  borderRadius: "50%",
  border: `1.5px solid ${champagne}`,
  color: "#f4c467",
  background: "radial-gradient(circle,#2a2216,#0c0f13 70%)",
  "@container": { [NARROW]: { width: "56px", height: "56px" } },
});
globalStyle(`${aiMark} svg`, { width: "52%", height: "52%" });
export const openMark = style([
  aiMark,
  { color: champagne, background: "transparent" },
]);
export const name = style({
  margin: 0,
  font: `27px/1.15 ${shellTokens.serif}`,
  color: shellTokens.ink,
  overflowWrap: "anywhere",
  "@container": { [NARROW]: { gridArea: "name", fontSize: "24px" } },
});
export const host = style({ color: champagne });
export const divider = style({
  alignSelf: "stretch",
  margin: "14px 0",
  background: rule,
  "@container": { [NARROW]: { display: "none" } },
});
export const controls = style({
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "12px 14px",
  minWidth: 0,
  "@container": { [NARROW]: { gridArea: "controls" } },
});
/** A compact select or chooser inside a seat. */
export const compact = style({ minWidth: 0 });
globalStyle(`${compact} > button`, {
  height: "42px",
  minHeight: 0,
  padding: "0 14px",
  fontSize: "21px",
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const passive = style({
  font: `22px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const waiting = style({
  marginLeft: "auto",
  font: `italic 22px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const warning = style({
  font: `italic 21px/1.2 ${shellTokens.serif}`,
  color: "#f0c674",
});
/** Opens Choose Deck: a field-like button with the deck icon and a chevron. */
export const deckButton = style({
  appearance: "none",
  display: "inline-flex",
  alignItems: "center",
  gap: "14px",
  maxWidth: "100%",
  height: "44px",
  padding: "0 16px",
  border: `1px solid ${t.color.formEdge}`,
  borderRadius: "10px",
  background: "#0b1016",
  color: shellTokens.ink,
  font: `22px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&:hover:not(:disabled)": { background: "#151b22" },
    "&:focus-visible": focus,
    "&:disabled": { opacity: 0.45, cursor: "default" },
    '&[data-needs="true"]': { borderColor: "#f0c674" },
  },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const deckLabel = style({
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
export const icon = style({ width: "22px", height: "22px", flexShrink: 0 });

/** Ready or Not ready, passive. Your own changes with the action below. */
export const readiness = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "12px",
  height: "44px",
  padding: "0 22px 0 16px",
  border: "1px solid #8b8478",
  borderRadius: "22px",
  font: `22px/1 ${shellTokens.serif}`,
  color: shellTokens.ink,
  whiteSpace: "nowrap",
  "@container": { [NARROW]: { gridArea: "ready", padding: "0 14px 0 12px" } },
  selectors: {
    '&[data-ready="true"]': {
      borderColor: "#5c8a3c",
      background: "linear-gradient(180deg,#1f3a16,#16290f)",
    },
  },
});
export const readyIcon = style({
  width: "22px",
  height: "22px",
  selectors: { [`${readiness}[data-ready="true"] &`]: { color: "#9ad66a" } },
});

export const side = style({
  display: "flex",
  flexDirection: "column",
  gap: "18px",
  "@container": {
    [STACKED]: {
      display: "grid",
      gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))",
      alignItems: "stretch",
      // Stacked, the deck box rises into space of its own instead of over the players.
      paddingTop: "112px",
    },
  },
});
export const deckPanel = style({
  ...panel,
  position: "relative",
  minHeight: "210px",
  marginTop: "6px",
  padding: "28px 32px",
  display: "grid",
  gridTemplateColumns: "minmax(0,1fr) 230px",
  alignItems: "end",
});
export const deckPanelLabel = style({
  position: "absolute",
  top: "28px",
  left: "32px",
  font: `24px/1 ${shellTokens.serif}`,
  color: "#e8dcc6",
});
export const deckName = style({
  margin: "0 0 4px",
  font: `30px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  overflowWrap: "anywhere",
});
export const deckFormat = style({
  margin: 0,
  font: `22px/1.2 ${shellTokens.serif}`,
  color: champagne,
});
/** The 3D box stands up out of the panel's top edge, as approved. */
export const box = style({
  position: "relative",
  width: "230px",
  height: "280px",
  marginTop: "-118px",
  marginBottom: "-34px",
  alignSelf: "start",
});
/** The box itself is a button: it reopens Choose Deck. */
export const boxButton = style({
  appearance: "none",
  display: "block",
  width: "100%",
  height: "100%",
  padding: 0,
  border: 0,
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
  borderRadius: "14px",
  selectors: { "&:focus-visible": focus },
});
export const boxStage = style({
  display: "block",
  position: "relative",
  width: "100%",
  height: "100%",
  pointerEvents: "none",
});
export const boxStatus = style({ display: "none" });
export const noDeck = style({
  margin: 0,
  font: `italic 22px/1.3 ${shellTokens.serif}`,
  color: "#d8ccb7",
});

export const rules = style({ ...panel, padding: "22px 32px 26px" });
export const rulesList = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr)",
  margin: 0,
});
globalStyle(`${rulesList} > dt, ${rulesList} > dd`, {
  margin: 0,
  padding: "6px 0",
  borderBottom: `1px solid ${rule}`,
  font: `21px/1.3 ${shellTokens.serif}`,
});
globalStyle(`${rulesList} > dt`, { color: champagne, paddingRight: "28px" });
globalStyle(`${rulesList} > dd`, { color: shellTokens.ink });
globalStyle(
  `${rulesList} > dt:nth-last-of-type(1), ${rulesList} > dd:last-of-type`,
  {
    borderBottom: 0,
  },
);
export const editRules = style({
  display: "flex",
  justifyContent: "center",
  marginTop: "22px",
});

export const notice = style({
  position: "relative",
  zIndex: 3,
  height: 0,
  marginTop: "48px",
  marginLeft: "auto",
  maxWidth: "520px",
});
export const footer = style({
  display: "flex",
  justifyContent: "flex-end",
  alignItems: "center",
  flexWrap: "wrap",
  gap: "24px",
  marginTop: "14px",
  selectors: {
    [phonePortrait]: {
      position: "sticky",
      bottom: "100px",
      marginTop: "28px",
      padding: "12px",
      borderRadius: "14px",
      background: "#0a0e13",
      border: `1px solid ${panelEdge}`,
    },
  },
});
globalStyle(`${footer} > button`, {
  minWidth: "240px",
  height: "54px",
  fontSize: "27px",
  "@media": {
    [touch]: { minWidth: "140px", minHeight: TOUCH_TARGET, height: "auto" },
  },
});
globalStyle(`:root[data-device="phone"] ${footer} > button`, {
  flex: 1,
  minWidth: 0,
});
export const countdown = style({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "12px",
  minWidth: "340px",
  paddingLeft: "24px",
  borderLeft: `1px solid ${rule}`,
  font: `28px/1.1 ${shellTokens.serif}`,
  color: champagne,
  selectors: {
    [phone]: { minWidth: 0, flex: 1, paddingLeft: "14px", fontSize: "22px" },
  },
});
export const bar = style({
  width: "100%",
  height: "16px",
  padding: "2px",
  borderRadius: "8px",
  border: `1px solid ${champagne}`,
  overflow: "hidden",
});
export const barFill = style({
  display: "block",
  height: "100%",
  borderRadius: "6px",
  background: "linear-gradient(180deg,#f7dc9d,#c99a4e)",
  transformOrigin: "left center",
});
const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  display: "inline-block",
  width: "30px",
  height: "30px",
  borderRadius: "50%",
  border: "3px solid #ffffff2b",
  borderTopColor: "#efe2c9",
  "@media": { [standardMotion]: { animation: `${spin} 1s linear infinite` } },
});
export const starting = style({
  display: "flex",
  alignItems: "center",
  gap: "18px",
  font: `28px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});

/** Lobby closed, full or unavailable: one card in the middle, with Back. */
export const gap = style({
  ...panel,
  maxWidth: "560px",
  margin: "12vh auto 0",
  padding: "40px 44px",
  textAlign: "center",
});
export const gapTitle = style({
  margin: 0,
  font: `36px/1.2 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
});
export const gapText = style({
  margin: "12px 0 28px",
  font: `23px/1.35 ${shellTokens.serif}`,
  color: "#e8dcc6",
});

/** Choose Deck and Invite players: one list body that scrolls inside fixed head and foot. */
export const chooser = style({
  selectors: { '&&:not([data-presentation="drawer"])': { width: "668px" } },
});
// The approved choosers centre their two actions.
globalStyle(`${chooser} > footer`, { justifyContent: "center" });
export const list = style({
  display: "flex",
  flexDirection: "column",
  height: "390px",
  marginTop: "16px",
  padding: "4px",
  overflowY: "auto",
  border: `1px solid ${rule}`,
  borderRadius: "10px",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  selectors: { [phone]: { height: "50vh" } },
});
export const option = style({
  appearance: "none",
  display: "grid",
  gridTemplateColumns: "88px 1px minmax(0,1fr) 32px",
  alignItems: "center",
  gap: "0 22px",
  width: "100%",
  minHeight: "82px",
  padding: "6px 18px 6px 14px",
  border: "1.5px solid transparent",
  borderBottomColor: rule,
  borderRadius: "10px",
  background: "none",
  color: shellTokens.ink,
  textAlign: "left",
  cursor: "pointer",
  flexShrink: 0,
  selectors: {
    "&:hover": { background: "#ffffff08" },
    "&:focus-visible": focus,
    '&[aria-checked="true"]': {
      borderColor: "#f2deb0",
      background: "#ffffff06",
    },
  },
});
export const thumb = style({
  width: "62px",
  height: "64px",
  justifySelf: "center",
  borderRadius: "6px",
  objectFit: "cover",
  boxShadow: "0 6px 14px #000a, inset 0 0 0 1px #ffffff22",
});
export const randomThumb = style([
  thumb,
  {
    display: "grid",
    placeItems: "center",
    background: "radial-gradient(circle,#2a2216,#0c0f13 70%)",
    color: "#f4c467",
  },
]);
export const optionDivider = style({
  alignSelf: "stretch",
  margin: "10px 0",
  background: rule,
});
export const optionText = style({
  display: "flex",
  flexDirection: "column",
  gap: "4px",
  minWidth: 0,
  font: `26px/1.1 ${shellTokens.serif}`,
});
export const optionSub = style({ fontSize: "21px", color: "#e8dcc6" });
export const radio = style({
  width: "28px",
  height: "28px",
  color: shellTokens.ink,
});
export const listState = style({
  display: "grid",
  placeContent: "center",
  justifyItems: "center",
  gap: "10px",
  flex: 1,
  padding: "20px",
  textAlign: "center",
  font: `23px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const listQuiet = style({ fontSize: "21px", color: "#cfc4b1" });
export const person = style([
  option,
  { gridTemplateColumns: "80px minmax(0,1fr) 32px" },
]);
export const tick = style({
  display: "grid",
  placeItems: "center",
  width: "32px",
  height: "32px",
  borderRadius: "6px",
  border: "1.5px solid #8b8478",
  selectors: {
    [`${person}[aria-checked="true"] &`]: {
      borderColor: "#7fb85a",
      background: "#1f3a16",
      color: "#b8f08c",
    },
  },
});
export const fieldError = style({
  margin: "10px 0 0",
  font: `italic 21px/1.3 ${shellTokens.serif}`,
  color: "#f0c674",
});

/** Game rules: a drawer from the right on wide screens. */
export const drawer = style({
  position: "fixed",
  inset: "auto 0 auto auto",
  top: "120px",
  right: "14px",
  width: "572px",
  maxWidth: "calc(100vw - 28px)",
  maxHeight: "calc(100vh - 140px)",
  margin: 0,
  padding: 0,
  border: `1.5px solid ${panelEdge}`,
  borderRadius: "16px",
  background: "#0b1016",
  color: shellTokens.ink,
  boxShadow: "0 30px 80px #000c",
  overflow: "hidden",
  selectors: {
    "&::backdrop": { background: "#04070b99" },
    "&[open]": { display: "flex", flexDirection: "column" },
  },
  "@media": {
    [standardMotion]: {
      selectors: {
        "&[open]": {
          animation: `${keyframes({ from: { transform: "translateX(40px)", opacity: 0 } })} ${m.duration.scene}s ${arrive}`,
        },
      },
    },
  },
});
export const drawerHead = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "28px 26px 18px 38px",
  borderBottom: `1px solid ${rule}`,
});
export const drawerTitle = style({
  margin: 0,
  font: `38px/1 ${shellTokens.serif}`,
  fontWeight: 400,
});
export const close = style({
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "48px",
  height: "48px",
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
export const drawerBody = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: "8px 26px 8px 38px",
});
export const field = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,1fr) 290px",
  alignItems: "center",
  gap: "16px",
  padding: "12px 0",
  borderBottom: `1px solid ${rule}`,
  font: `24px/1.2 ${shellTokens.serif}`,
  selectors: {
    [phone]: { gridTemplateColumns: "minmax(0,1fr) minmax(0,1.3fr)" },
  },
});
globalStyle(`${field} button`, { width: "100%" });
export const advanced = style({
  margin: "18px 0 8px",
  border: `1px solid ${rule}`,
  borderRadius: "12px",
  background: "#ffffff05",
});
export const advancedToggle = style({
  appearance: "none",
  display: "flex",
  alignItems: "center",
  gap: "18px",
  width: "100%",
  padding: "16px 22px",
  border: 0,
  background: "none",
  color: shellTokens.ink,
  font: `26px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: { "&:focus-visible": focus },
});
export const advancedBody = style({ padding: "0 22px 6px" });
export const toggle = style({
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  padding: "3px",
  border: `1px solid ${t.color.formEdge}`,
  borderRadius: "22px",
  background: "#0b1016",
});
export const toggleOption = style({
  appearance: "none",
  height: "38px",
  border: 0,
  borderRadius: "19px",
  background: "none",
  color: shellTokens.ink,
  font: `22px/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&:focus-visible": focus,
    '&[aria-checked="true"]': {
      background: "linear-gradient(180deg,#5a4d35,#3b3223)",
      boxShadow: `inset 0 0 0 1px ${champagne}`,
    },
  },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const drawerFoot = style({
  display: "grid",
  gridTemplateColumns: "1fr 1fr",
  gap: "24px",
  padding: "20px 26px 24px 38px",
  borderTop: `1px solid ${rule}`,
});
globalStyle(`${drawerFoot} > button`, { width: "100%" });
