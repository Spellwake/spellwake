import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * Market Supply, as approved (SP-01 to SP-10): the vote banner, search and
 * Availability, a gallery of pack-product tiles three across, and the Buy
 * packs and Supply vote dialogs. Lengths are in the approved references'
 * pixel scale. The gallery narrows to two across, then one, by its own width.
 */
const TWO = "supply (inline-size < 64rem)";
const ONE = "supply (inline-size < 40rem)";
const phone = ':root[data-device="phone"] &';
const compact = ':root[data-shell="compact"] &';
const champagne = "#e7cf9f";
const edge = "#cdb48a";
const rule = "#a698774c";
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;

export const page = style({
  containerName: "supply",
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  padding: "30px 3% 40px 32px",
  selectors: {
    [compact]: { paddingBottom: "130px" },
    [phone]: { padding: "16px 16px 150px" },
  },
});

export const banner = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr) auto",
  alignItems: "center",
  gap: "8px 34px",
  padding: "18px 26px",
  border: `1.5px solid ${edge}`,
  borderRadius: "12px",
  background: "linear-gradient(180deg,#0d1218e6,#0a0e13f0)",
  boxShadow: "0 16px 40px #0007",
  "@container": {
    [ONE]: {
      gridTemplateColumns: "auto minmax(0,1fr)",
      gap: "12px 18px",
      padding: "16px",
    },
  },
});
export const bannerIcon = style({
  width: "44px",
  height: "44px",
  color: champagne,
  paddingRight: "30px",
  borderRight: `1px solid ${rule}`,
  boxSizing: "content-box",
  "@container": { [ONE]: { paddingRight: 0, borderRight: 0 } },
});
export const bannerCopy = style({ minWidth: 0 });
export const bannerTitle = style({
  margin: 0,
  font: `32px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  "@container": { [ONE]: { fontSize: "26px" } },
});
export const bannerDetail = style({
  margin: "6px 0 0",
  font: `22px/1.25 ${shellTokens.serif}`,
  color: champagne,
  whiteSpace: "pre",
  "@container": { [ONE]: { whiteSpace: "normal", fontSize: "20px" } },
});
export const bannerAction = style({
  whiteSpace: "nowrap",
  "@container": { [ONE]: { gridColumn: "1 / -1" } },
});

export const toolbar = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,458px) minmax(0,260px)",
  gap: "30px",
  margin: "24px 0 28px",
  "@container": {
    [ONE]: {
      gridTemplateColumns: "minmax(0,1fr)",
      gap: "12px",
      margin: "18px 0 20px",
    },
  },
});
export const search = style({ minWidth: 0 });

export const gallery = style({
  listStyle: "none",
  margin: 0,
  padding: 0,
  display: "grid",
  gridTemplateColumns: "repeat(3, minmax(0,1fr))",
  gap: "44px 28px",
  "@container": {
    [TWO]: { gridTemplateColumns: "repeat(2, minmax(0,1fr))" },
    [ONE]: { gridTemplateColumns: "minmax(0,1fr)", gap: "26px" },
  },
});
export const tile = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,152px) minmax(0,1fr)",
  alignItems: "start",
  // Narrower where three tiles share a tablet's width, so a set name keeps its words whole.
  gap: "clamp(14px, 1.9cqi, 30px)",
  minWidth: 0,
  selectors: { '&[data-kind="set"]': { gridTemplateColumns: "minmax(0,1fr)" } },
});
export const art = style({ display: "block", minWidth: 0 });
export const copy = style({
  display: "grid",
  alignContent: "start",
  gap: "10px",
  minWidth: 0,
  paddingTop: "22px",
});
export const name = style({
  margin: 0,
  display: "grid",
  gap: "6px",
  font: `28px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  overflowWrap: "break-word",
  hyphens: "manual",
});
export const product = style({ fontSize: "21px", color: "#e3dccb" });
export const remaining = style({
  margin: "6px 0 0",
  font: `21px/1.2 ${shellTokens.serif}`,
  color: "#e3dccb",
});
export const checking = style({
  margin: "6px 0 0",
  font: `italic 21px/1.2 ${shellTokens.serif}`,
  color: "#e3dccb",
});
export const price = style({
  margin: 0,
  fontSize: "30px",
  color: shellTokens.ink,
});
export const tileAction = style({
  justifySelf: "start",
  selectors: { "&&": { minWidth: "152px", fontSize: "23px" } },
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
});
/** A set with no qualified product: its code and name, quietly, with Unavailable. */
export const bare = style({
  display: "grid",
  justifyItems: "center",
  alignContent: "center",
  gap: "14px",
  minHeight: "250px",
  textAlign: "center",
});
export const bareCode = style({
  font: `48px/1 ${shellTokens.serif}`,
  color: "#c9bfae",
  letterSpacing: "0.02em",
});
export const bareName = style({
  margin: 0,
  font: `25px/1.2 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
});
globalStyle(`${bare} ${tileAction}`, {
  justifySelf: "center",
  minWidth: "220px",
});

const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  display: "inline-block",
  flex: "none",
  width: "30px",
  height: "30px",
  borderRadius: "50%",
  border: "3px solid #ffffff26",
  borderTopColor: "#e9dcc4",
  "@media": { [standardMotion]: { animation: `${spin} 1s linear infinite` } },
});
export const smallSpinner = style([spinner, { width: "22px", height: "22px" }]);
export const appending = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "14px",
  padding: "34px 0 0",
  font: `22px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const quiet = style({
  display: "grid",
  placeItems: "center",
  minHeight: "40vh",
  // The library's empty states size their words from here: a 28px title.
  fontSize: "12px",
});
export const loading = style({
  display: "flex",
  alignItems: "center",
  gap: "16px",
  font: `24px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});

/** Buy packs. */
export const buyItem = style({
  display: "grid",
  gridTemplateColumns: "minmax(0,150px) minmax(0,1fr)",
  gap: "28px",
  alignItems: "start",
  padding: "10px 0 16px",
  selectors: {
    [phone]: {
      gridTemplateColumns: "minmax(0,110px) minmax(0,1fr)",
      gap: "18px",
    },
  },
});
export const buyArt = style({ display: "block" });
export const buyCopy = style({
  display: "grid",
  gap: "8px",
  paddingTop: "24px",
  font: `24px/1.25 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
globalStyle(`${buyCopy} p`, { margin: 0 });
export const buyName = style({ fontSize: "32px" });
export const row = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "16px",
  padding: "12px 4px",
  borderTop: `1px solid ${rule}`,
  font: `26px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const rowLabel = style({ flex: "none" });
export const rowControl = style({ minWidth: 0 });
export const rowError = style({
  margin: "0 4px 6px",
  font: `21px/1.25 ${shellTokens.serif}`,
  color: "#ff9d7a",
});
export const changed = style({
  margin: "0 4px",
  padding: "8px 0 0",
  borderTop: `1px solid ${rule}`,
  font: `italic 21px/1.25 ${shellTokens.serif}`,
  color: champagne,
});
export const total = style({
  display: "flex",
  alignItems: "center",
  gap: "14px",
  fontSize: "30px",
});
export const totalNote = style({
  font: `italic 20px/1.2 ${shellTokens.serif}`,
  color: "#d8cfbd",
});
export const lastKnown = style({ color: "#b9b2a3" });
export const struck = style({ color: "#b9b2a3", fontSize: "24px" });
export const stageLine = style({
  margin: "12px 0 0",
  textAlign: "center",
  font: `22px/1.3 ${shellTokens.serif}`,
  selectors: {
    '&[data-tone="unknown"]': { color: "#e2c27f" },
    '&[data-tone="known"]': { color: shellTokens.ink },
  },
});

/** Supply vote. */
export const vote = style({
  display: "grid",
  gap: "18px",
  minWidth: 0,
});
export const voteHead = style({
  margin: 0,
  display: "flex",
  justifyContent: "space-between",
  gap: "16px",
  flexWrap: "wrap",
  paddingBottom: "12px",
  borderBottom: `1px solid ${rule}`,
  font: `24px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const candidates = style({
  listStyle: "none",
  margin: 0,
  padding: 0,
  display: "grid",
  gridTemplateColumns: "repeat(5, minmax(0,1fr))",
  gap: "14px",
  maxHeight: "min(330px, 46dvh)",
  overflowY: "auto",
  overscrollBehavior: "contain",
  "@media": {
    "(max-width: 900px)": { gridTemplateColumns: "repeat(3, minmax(0,1fr))" },
  },
  // The list scrolls in its own bounded area, as approved, so the count and Save stay below it.
  selectors: {
    [phone]: {
      gridTemplateColumns: "repeat(2, minmax(0,1fr))",
      maxHeight: "min(330px, 40dvh)",
    },
  },
});
export const placeholder = style({
  height: "132px",
  borderRadius: "10px",
  background: "linear-gradient(180deg,#2a2e33,#1d2126)",
});
export const candidate = style({
  appearance: "none",
  position: "relative",
  width: "100%",
  minHeight: "132px",
  display: "grid",
  alignContent: "center",
  justifyItems: "center",
  gap: "8px",
  padding: "30px 10px 16px",
  border: `1px solid #6e6656`,
  borderRadius: "10px",
  background: "linear-gradient(180deg,#10151b,#0b0f14)",
  color: shellTokens.ink,
  textAlign: "center",
  cursor: "pointer",
  selectors: {
    '&[aria-checked="true"]': {
      borderColor: champagne,
      boxShadow: `inset 0 0 0 1px ${champagne}55`,
      background: "linear-gradient(180deg,#1c1f22,#141719)",
    },
    "&:disabled": { opacity: 0.55, cursor: "default" },
    "&:focus-visible": focus,
  },
});
export const box = style({
  position: "absolute",
  top: "12px",
  left: "12px",
  display: "grid",
  placeItems: "center",
  width: "26px",
  height: "26px",
  border: `1.5px solid ${champagne}`,
  borderRadius: "4px",
  color: "#1a1408",
  selectors: {
    [`${candidate}[aria-checked="true"] &`]: { background: champagne },
  },
});
globalStyle(`${box} svg`, { width: "20px", height: "20px", strokeWidth: 3 });
export const candidateCode = style({
  font: `19px/1 ${shellTokens.serif}`,
  letterSpacing: "0.04em",
  color: "#d8cfbd",
});
export const candidateName = style({
  font: `23px/1.15 ${shellTokens.serif}`,
  overflowWrap: "anywhere",
});
export const voteNoMatch = style({
  margin: 0,
  padding: "40px 0",
  textAlign: "center",
  font: `italic 24px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const voteFoot = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "16px",
  flexWrap: "wrap",
  paddingTop: "14px",
  borderTop: `1px solid ${rule}`,
  font: `24px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
export const voteFootNote = style([voteFoot, { margin: 0 }]);
export const saveVote = style({
  selectors: { "&&": { minWidth: "230px", fontSize: "24px" } },
  "@media": {
    "(pointer: coarse)": { selectors: { "&&": { minHeight: "44PX" } } },
  },
});
export const voteQuiet = style({
  display: "grid",
  justifyItems: "center",
  gap: "14px",
  padding: "60px 0",
  font: `26px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
globalStyle(`${voteQuiet} p`, { margin: 0 });
export const voteQuietIcon = style({
  width: "48px",
  height: "48px",
  color: champagne,
});
export const results = style({
  width: "100%",
  borderCollapse: "collapse",
  border: `1px solid ${rule}`,
  borderRadius: "10px",
  font: `24px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
globalStyle(`${results} th, ${results} td`, {
  padding: "12px 20px",
  textAlign: "left",
  fontWeight: 400,
  borderBottom: `1px solid ${rule}`,
});
globalStyle(`${results} thead th`, { background: "#1a1e22", color: "#e3dccb" });
globalStyle(`${results} td`, { textAlign: "center" });
globalStyle(`${results} thead th:not(:first-child)`, { textAlign: "center" });
