import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * A player's public profile, as approved (PP-01 to PP-06). Lengths are in the
 * pixel scale of the approved references. Below `NARROW` the identity stacks
 * and the record shortens to wins, losses and draws by letter.
 */
const NARROW = "profile (inline-size < 44rem)";
/** Narrower than this, a format's rank is drawn as two lines instead of a table row. */
const ROWS = "profile (inline-size < 36rem)";
const rule = "#a698774c";
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;

export const shell = style({
  selectors: { "&&": { width: "1115px" } },
});
export const unavailableShell = style({
  selectors: { "&&": { width: "712px" } },
});
globalStyle(
  `${shell} > [data-dialog-body], ${unavailableShell} > [data-dialog-body]`,
  {
    display: "flex",
    flexDirection: "column",
    padding: 0,
  },
);
export const profile = style({
  containerName: "profile",
  containerType: "inline-size",
  position: "relative",
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  padding: "34px 36px 38px",
});
export const identity = style({
  flex: "none",
  display: "flex",
  alignItems: "center",
  gap: "40px",
  // Keeps the name and the action clear of the close button.
  padding: "0 60px 22px 36px",
  "@container": {
    // On a phone the picture is smaller, with the name and Message beside it.
    [NARROW]: {
      display: "grid",
      gridTemplateColumns: "auto minmax(0, 1fr)",
      gridTemplateRows: "1fr auto",
      alignItems: "center",
      gap: "8px 20px",
      padding: "0 52px 18px 0",
    },
  },
});
globalStyle(`${identity} > :first-child`, {
  "@container": {
    [NARROW]: { gridRow: "1 / -1", width: "132px", height: "132px" },
  },
});
export const username = style({
  flex: 1,
  minWidth: 0,
  margin: 0,
  font: `64px/1.1 ${shellTokens.serif}`,
  fontWeight: 600,
  color: shellTokens.ink,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  "@container": {
    [NARROW]: { fontSize: "34px", alignSelf: "end" },
  },
  selectors: {
    // With no Message under it, the name sits level with the picture's middle.
    "&:last-child": {
      "@container": { [NARROW]: { gridRow: "1 / -1", alignSelf: "center" } },
    },
  },
});
export const message = style({
  flex: "none",
  selectors: {
    "&&": { minHeight: "66px", padding: "0 38px", fontSize: "29px" },
  },
  "@container": {
    [NARROW]: {
      selectors: {
        "&&": {
          justifySelf: "start",
          alignSelf: "start",
          minHeight: "48px",
          padding: "0 22px",
          fontSize: "22px",
        },
      },
    },
  },
});
/** The rule under the identity, with its small centre diamond. */
export const flourish = style({
  flex: "none",
  position: "relative",
  height: "1px",
  background: `linear-gradient(to right, #c9b58a66, ${shellTokens.line} 50%, #c9b58a66)`,
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      left: "50%",
      top: "50%",
      width: "9px",
      height: "9px",
      transform: "translate(-50%, -50%) rotate(45deg)",
      background: shellTokens.line,
      boxShadow: `0 0 10px ${shellTokens.glow}`,
    },
  },
});
export const season = style({
  flex: "none",
  margin: 0,
  padding: "22px 8px 18px",
  font: `33px/1.15 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": { [NARROW]: { fontSize: "26px", padding: "16px 0 12px" } },
});
export const ranks = style({
  // Three rows show under the headings. A longer season scrolls here, so the
  // profile keeps one size whoever it shows.
  flex: "0 1 292px",
  minHeight: "170px",
  overflowY: "auto",
  "@container": {
    // Two-line rows are taller, so the list takes what the screen allows.
    [ROWS]: { flexBasis: "auto", maxHeight: "44dvh" },
  },
  borderTop: `1px solid ${rule}`,
  borderBottom: `1px solid ${rule}`,
  scrollbarWidth: "thin",
  scrollbarColor: "#8d8168 transparent",
  selectors: { "&:focus-visible": focus },
});
export const table = style({
  width: "100%",
  borderCollapse: "separate",
  borderSpacing: 0,
  font: `29px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  "@container": { [NARROW]: { fontSize: "22px" } },
});
globalStyle(`${table} th, ${table} td`, {
  padding: "0 12px",
  textAlign: "center",
  whiteSpace: "nowrap",
});
globalStyle(`${table} th:first-child, ${table} td:first-child`, {
  paddingLeft: "38px",
  textAlign: "start",
});
globalStyle(`${table} th`, {
  position: "sticky",
  top: 0,
  zIndex: 1,
  height: "52px",
  fontSize: "0.86em",
  fontWeight: 400,
  background: "#0b1218",
  borderBottom: `1px solid #d9c9a8a6`,
});
globalStyle(`${table} td`, { height: "80px" });
globalStyle(`${table} tbody tr + tr td`, { borderTop: `1px solid ${rule}` });
globalStyle(`${table} td:nth-child(2)`, { textAlign: "start" });
globalStyle(`${table} th:first-child, ${table} td:first-child`, {
  "@container": { [NARROW]: { paddingLeft: "4px" } },
});
globalStyle(`${table} td`, { "@container": { [NARROW]: { height: "62px" } } });
// A phone has no room for four columns. Each format becomes two lines there:
// its name and rating, then its rank and record. The headings stay for
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
      gridTemplateAreas: '"format rating" "rank record"',
      alignItems: "center",
      gap: "2px 16px",
      padding: "14px 4px",
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
    [ROWS]: {
      display: "block",
      height: "auto",
      padding: 0,
      border: 0,
      textAlign: "start",
    },
  },
});
globalStyle(`${table} tbody td:nth-child(1)`, {
  "@container": { [ROWS]: { gridArea: "format" } },
});
globalStyle(`${table} tbody td:nth-child(2)`, {
  "@container": { [ROWS]: { gridArea: "rank", minWidth: 0 } },
});
globalStyle(`${table} tbody td:nth-child(3)`, {
  "@container": { [ROWS]: { gridArea: "rating", justifySelf: "end" } },
});
globalStyle(`${table} tbody td:nth-child(4)`, {
  "@container": {
    [ROWS]: { gridArea: "record", justifySelf: "end", color: "#d8cdb9" },
  },
});
/** The record written out in words, and its short form for a narrow table. */
export const long = style({ "@container": { [ROWS]: { display: "none" } } });
export const short = style({
  display: "none",
  "@container": { [ROWS]: { display: "inline" } },
});
export const note = style({
  display: "grid",
  placeItems: "center",
  minHeight: "200px",
  margin: 0,
  font: `italic 29px/1.3 ${shellTokens.serif}`,
  color: "#d8cdb9",
});
export const skeleton = style({
  display: "grid",
  gap: "30px",
  padding: "26px 38px",
});
export const unavailable = style({
  display: "grid",
  placeItems: "center",
  minHeight: "168px",
  margin: 0,
  padding: "0 56px",
  font: `40px/1.2 ${shellTokens.serif}`,
  textAlign: "center",
});
/** One line of passive feedback, in the profile's lower corner. */
export const notice = style({
  position: "absolute",
  right: "22px",
  bottom: "14px",
  width: "min(calc(100% - 44px), 520px)",
  pointerEvents: "none",
});
