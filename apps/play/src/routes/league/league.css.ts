import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * League, as approved (LS-01 to LS-05, ST-01 to ST-05): the title and season
 * line, This season's table of every format, and Standings' format tabs over
 * a table whose body scrolls under fixed headings, with your own row pinned
 * below it until it scrolls into view. Lengths are in the approved
 * references' pixel scale. On a narrow page each row stacks its facts.
 */
const page = "league";
const NARROW = `${page} (inline-size < 46rem)`;
/** Below this This season stacks each format's facts (tablets and phones). */
const STACK = `${page} (inline-size < 88rem)`;
/** Below this the standings' rank shows as its emblem alone. */
const TIGHT = `${page} (inline-size < 62rem)`;
const phone = ':root[data-device="phone"] &';
const compact = ':root[data-shell="compact"] &';
/** A sideways phone is too short for a fixed head and a scrolling list: the page scrolls instead. */
const phoneLandscape =
  ':root[data-device="phone"][data-orientation="landscape"] &';
const champagne = "#e7cf9f";
const gold = "#d9b878";
const edge = "#cdb48a";
const rule = "#a698774c";
const muted = "#b9ae99";
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;

export const root = style({
  containerName: page,
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  padding: "26px 3% 28px 32px",
  selectors: {
    [compact]: { paddingBottom: "120px" },
    [phone]: { padding: "14px 16px 140px" },
    [phoneLandscape]: {
      display: "block",
      overflowY: "auto",
      padding: "10px 16px 40px",
    },
  },
});
export const title = style({
  position: "relative",
  alignSelf: "flex-start",
  margin: 0,
  font: `52px/1.1 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  "@container": { [NARROW]: { fontSize: "38px" } },
});
export const trailSpace = style({
  "@container": { [NARROW]: { display: "none" } },
});
export const seasonLine = style({
  margin: "8px 0 0",
  font: `24px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": { [NARROW]: { fontSize: "19px" } },
});
export const seasonName = style({ color: champagne });

/** The panel holding a table; its body scrolls under fixed headings. */
export const panel = style({
  selectors: { [phoneLandscape]: { display: "block" } },
  flex: "0 1 auto",
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  marginTop: "26px",
  padding: "6px 18px 8px",
  border: `1.5px solid ${edge}`,
  borderRadius: "16px",
  background: "linear-gradient(180deg,#0d1218e6,#0a0e13f0)",
  boxShadow: "0 16px 40px #0007",
  "@container": { [NARROW]: { padding: "4px 12px 6px" } },
});
export const tabbedPanel = style([
  panel,
  { marginTop: 0, borderTopLeftRadius: 0, borderTopRightRadius: 0 },
]);
const seasonColumns =
  "minmax(0,1fr) minmax(0,1.25fr) minmax(0,1.3fr) minmax(0,1.55fr) minmax(0,0.85fr)";
const standingColumns =
  "minmax(0,0.6fr) minmax(0,2fr) minmax(0,1.3fr) minmax(0,0.9fr) minmax(0,0.9fr)";
export const headRow = style({
  flexShrink: 0,
  display: "grid",
  alignItems: "center",
  gap: "16px",
  padding: "14px 22px",
  borderBottom: `1px solid ${rule}`,
  font: `21px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  selectors: {
    '&[data-table="season"]': { gridTemplateColumns: seasonColumns },
    '&[data-table="standings"]': { gridTemplateColumns: standingColumns },
  },
  "@container": {
    [STACK]: {
      selectors: {
        '&[data-table="season"]': {
          position: "absolute",
          width: "1px",
          height: "1px",
          overflow: "hidden",
          clip: "rect(0 0 0 0)",
          padding: 0,
        },
      },
    },
    [NARROW]: {
      selectors: {
        '&[data-table="standings"]': {
          gridTemplateColumns: "34px minmax(0,1fr) 44px 64px 74px",
          gap: "8px",
          padding: "10px 8px",
          fontSize: "17px",
        },
      },
    },
  },
});
export const end = style({ textAlign: "right" });
export const center = style({ textAlign: "center" });
export const body = style({
  selectors: { [phoneLandscape]: { overflow: "visible" } },
  flex: "0 1 auto",
  minHeight: 0,
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
});
globalStyle(`${body}:focus-visible`, focus);

export const seasonRow = style({
  display: "grid",
  gridTemplateColumns: seasonColumns,
  alignItems: "center",
  gap: "16px",
  minHeight: "100px",
  padding: "8px 22px",
  borderBottom: `1px solid ${rule}`,
  selectors: { "&:last-child": { borderBottom: 0 } },
  "@container": {
    [STACK]: {
      gridTemplateColumns: "minmax(0,1fr) auto",
      gridTemplateAreas: '"format reward" "rank record" "progress progress"',
      gap: "10px 12px",
      padding: "16px 6px",
    },
  },
});
export const format = style({
  font: `30px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  overflowWrap: "normal",
  "@container": { [STACK]: { gridArea: "format", fontSize: "26px" } },
});
export const rankCell = style({
  minWidth: 0,
  "@container": { [STACK]: { gridArea: "rank" } },
});
export const progressCell = style({
  minWidth: 0,
  "@container": { [STACK]: { gridArea: "progress" } },
});
export const recordCell = style({
  font: `22px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": {
    [STACK]: { gridArea: "record", textAlign: "right", fontSize: "18px" },
  },
});
export const rewardCell = style({
  display: "flex",
  justifyContent: "flex-end",
  font: `30px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": { [STACK]: { gridArea: "reward", fontSize: "24px" } },
});
export const none = style({ color: muted, paddingRight: "18px" });

/** Format tabs: a strip that scrolls sideways when the formats outgrow it. */
export const tabs = style({
  flexShrink: 0,
  display: "flex",
  marginTop: "22px",
  padding: "0 6px",
  border: `1.5px solid ${edge}`,
  borderBottom: 0,
  borderRadius: "16px 16px 0 0",
  background: "linear-gradient(180deg,#0d1218e6,#0a0e13f0)",
  overflowX: "auto",
  scrollbarWidth: "thin",
  scrollbarColor: `${gold} #ffffff14`,
});
export const tab = style({
  position: "relative",
  flex: "1 0 auto",
  minWidth: "150px",
  minHeight: "44PX",
  padding: "14px 26px 16px",
  border: 0,
  borderRadius: "14px 14px 0 0",
  background: "none",
  font: `27px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  cursor: "pointer",
  whiteSpace: "nowrap",
  transition: `color 180ms ease-out`,
  selectors: {
    "&:hover": { color: champagne },
    "&:focus-visible": { ...focus, outlineOffset: "-3px" },
    '&[aria-selected="true"]': {
      color: champagne,
      background:
        "linear-gradient(180deg,#3a2f1c 0%,#1b170f 70%,transparent 100%)",
      boxShadow: `inset 0 1px 0 ${edge}99`,
    },
    '&[aria-selected="true"]::after': {
      content: '""',
      position: "absolute",
      left: "20%",
      right: "20%",
      bottom: "4px",
      height: "3px",
      borderRadius: "3px",
      background: `linear-gradient(90deg,transparent,${gold},transparent)`,
      boxShadow: `0 0 12px ${gold}`,
    },
  },
  "@container": {
    [NARROW]: { minWidth: "110px", fontSize: "21px", padding: "12px 16px" },
  },
});

export const standingRow = style({
  display: "grid",
  gridTemplateColumns: standingColumns,
  alignItems: "center",
  gap: "16px",
  minHeight: "90px",
  padding: "6px 22px",
  borderBottom: `1px solid ${rule}`,
  font: `29px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  scrollMarginBlock: "8px",
  selectors: {
    '&[data-you="true"]': {
      borderRadius: "10px",
      border: `1px solid ${edge}aa`,
      background: "linear-gradient(180deg,#3a3220e6,#211d14f0)",
    },
  },
  "@container": {
    [NARROW]: {
      gridTemplateColumns: "34px minmax(0,1fr) 44px 64px 74px",
      gap: "8px",
      minHeight: "64px",
      padding: "6px 8px",
      fontSize: "20px",
    },
  },
});
export const place = style({ textAlign: "center" });
/** Your row below the list while it is out of view; its space stays when hidden. */
export const pinned = style([
  standingRow,
  {
    flexShrink: 0,
    marginTop: "10px",
    selectors: { '&[data-hidden="true"]': { visibility: "hidden" } },
  },
]);
export const player = style({
  display: "inline-flex",
  alignItems: "center",
  gap: "22px",
  minWidth: 0,
  maxWidth: "100%",
  minHeight: "44PX",
  padding: 0,
  border: 0,
  background: "none",
  font: "inherit",
  color: "inherit",
  textAlign: "left",
  cursor: "pointer",
  selectors: {
    "&:hover": { color: champagne },
    "&:focus-visible": { ...focus, borderRadius: "8px" },
  },
  "@container": { [NARROW]: { gap: "10px" } },
});
export const playerName = style({
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});
/** Below the narrow width the rank shows as its emblem alone. */
export const rankStanding = style({
  minWidth: 0,
  "@container": {
    [TIGHT]: { display: "flex", justifyContent: "center" },
  },
});
globalStyle(`${rankStanding} img`, {
  "@container": { [NARROW]: { width: "36px", height: "36px" } },
});
globalStyle(`${rankStanding} [data-presentation] > span:first-child`, {
  "@container": { [NARROW]: { width: "36px", height: "36px" } },
});
globalStyle(`${rankStanding} [data-presentation] > span:last-child`, {
  "@container": { [TIGHT]: { display: "none" } },
});

export const quiet = style({
  margin: 0,
  padding: "40px 24px",
  textAlign: "center",
  font: `italic 28px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": { [NARROW]: { fontSize: "22px", padding: "28px 12px" } },
});
export const neutral = style({ minHeight: "220px" });

const shimmer = keyframes({
  "0%": { backgroundPosition: "100% 0" },
  "100%": { backgroundPosition: "-100% 0" },
});
export const skeleton = style({
  display: "block",
  height: "14px",
  borderRadius: "7px",
  background:
    "linear-gradient(90deg,#ffffff0d 30%,#ffffff1f 50%,#ffffff0d 70%)",
  backgroundSize: "200% 100%",
  "@media": {
    [standardMotion]: { animation: `${shimmer} 1.6s linear infinite` },
  },
});
export const skeletonDot = style([
  skeleton,
  { width: "40px", height: "40px", borderRadius: "50%" },
]);
export const skeletonGem = style([
  skeleton,
  {
    width: "30px",
    height: "30px",
    borderRadius: "4px",
    transform: "rotate(45deg)",
  },
]);
export const skeletonRow = style({ minHeight: "64px" });
export const srOnly = style({
  position: "absolute",
  width: "1px",
  height: "1px",
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
});
/** The passive notice sits at the panel's foot, inside its corner. */
export const notice = style({
  display: "flex",
  justifyContent: "flex-end",
  padding: "8px 4px 4px",
  fontSize: "13px",
});
