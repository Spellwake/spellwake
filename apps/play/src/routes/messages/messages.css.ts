import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/**
 * Messages, as approved (messaging-approved-v1, MSG-S01 and MSG-D01 to D13):
 * a panel under the header's Messages button, friends on a rail at its
 * left, the open person's thread, Requests or Find People on its right.
 * Lengths are in the pixel scale of the approved references.
 */
const champagne = "#e7cf9f";
const edge = "#cdb48a66";
const rule = "#a698774c";
const arrive = cssEase(m.ease.arrive);
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "2px",
} as const;
const rise = keyframes({
  from: { opacity: 0, transform: "translateY(-10px) scale(.985)" },
});

/** The panel under the header button on wide screens. */
export const panel = style({
  position: "fixed",
  zIndex: 40,
  top: "76px",
  right: "24px",
  width: "532px",
  height: "548px",
  maxHeight: "calc(100vh - 100px)",
  display: "grid",
  gridTemplateColumns: "80px minmax(0,1fr)",
  border: `1px solid ${edge}`,
  borderRadius: "14px",
  background: "linear-gradient(180deg,#121519f7,#0d1014fa)",
  boxShadow: "0 30px 80px #000c",
  overflow: "hidden",
  transformOrigin: "88% 0",
  "@media": {
    [standardMotion]: { animation: `${rise} ${m.duration.scene}s ${arrive}` },
  },
});
/** Inside the phone drawer the same layout fills the sheet. */
export const sheet = style({
  display: "grid",
  gridTemplateColumns: "72px minmax(0,1fr)",
  height: "min(78vh, 760px)",
  margin: "-8px -16px -16px",
});

export const rail = style({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "12px",
  padding: "18px 0 18px",
  borderRight: `1px solid ${rule}`,
  minHeight: 0,
});
export const friends = style({
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "14px",
  width: "100%",
  padding: "4px 0",
  overflowY: "auto",
  scrollbarWidth: "none",
});
export const railButton = style({
  appearance: "none",
  position: "relative",
  display: "grid",
  placeItems: "center",
  width: "60px",
  height: "60px",
  flexShrink: 0,
  padding: 0,
  border: "2px solid transparent",
  borderRadius: "50%",
  background: "none",
  color: shellTokens.ink,
  cursor: "pointer",
  selectors: {
    "&:focus-visible": focus,
    '&[aria-current="true"]': {
      borderColor: champagne,
      boxShadow: `0 0 18px #e7cf9f55`,
    },
  },
  "@media": {
    [touch]: {
      minWidth: `max(60px, ${TOUCH_TARGET})`,
      minHeight: `max(60px, ${TOUCH_TARGET})`,
    },
  },
});
export const railDivider = style({
  width: "36px",
  height: "1px",
  background: rule,
  flexShrink: 0,
});
export const roundIcon = style([
  railButton,
  {
    width: "46px",
    height: "46px",
    border: `1px solid ${champagne}99`,
    selectors: {
      '&[aria-current="true"]': {
        borderColor: champagne,
        background: "#e7cf9f1f",
      },
    },
    "@media": {
      [touch]: { minWidth: TOUCH_TARGET, minHeight: TOUCH_TARGET },
    },
  },
]);
globalStyle(`${roundIcon} svg`, { width: "24px", height: "24px" });
export const badge = style({
  position: "absolute",
  top: "-6px",
  right: "-6px",
  minWidth: "22px",
  height: "22px",
  padding: "0 5px",
  borderRadius: "11px",
  background: "linear-gradient(180deg,#f0c873,#c9963f)",
  color: "#1b1307",
  font: `600 15px/22px ${shellTokens.serif}`,
  textAlign: "center",
});
export const unreadDot = style({
  position: "absolute",
  top: "2px",
  right: "2px",
  width: "12px",
  height: "12px",
  borderRadius: "50%",
  background: "#f0c873",
  boxShadow: "0 0 0 2px #0d1014",
});

export const main = style({
  minWidth: 0,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
});
export const head = style({
  display: "flex",
  alignItems: "flex-start",
  gap: "8px",
  padding: "18px 14px 12px 26px",
  borderBottom: `1px solid ${rule}`,
});
export const headText = style({ flex: 1, minWidth: 0 });
/** Space under the phone drawer's floating close button. */
export const closeRoom = style({ width: "52px", flexShrink: 0 });
export const title = style({
  margin: 0,
  font: `27px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  overflowWrap: "anywhere",
});
export const relation = style({
  display: "flex",
  alignItems: "center",
  gap: "8px",
  margin: "2px 0 0",
  font: `20px/1.2 ${shellTokens.serif}`,
  color: champagne,
});
export const iconButton = style({
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "40px",
  height: "40px",
  flexShrink: 0,
  border: 0,
  borderRadius: "8px",
  background: "none",
  color: shellTokens.ink,
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#ffffff12" },
    "&:focus-visible": focus,
  },
  "@media": { [touch]: { width: TOUCH_TARGET, height: TOUCH_TARGET } },
});
globalStyle(`${iconButton} svg`, { width: "24px", height: "24px" });

export const body = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: "10px 16px 8px 18px",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
});
export const day = style({
  display: "flex",
  alignItems: "center",
  gap: "16px",
  margin: "8px 0 14px",
  font: `17px/1 ${shellTokens.serif}`,
  color: "#b7ad9c",
  selectors: {
    "&::before, &::after": {
      content: '""',
      flex: 1,
      height: "1px",
      background: rule,
    },
  },
});
export const message = style({
  display: "grid",
  gridTemplateColumns: "44px minmax(0,1fr) auto",
  alignItems: "start",
  gap: "0 10px",
  margin: "0 0 12px",
});
export const mine = style({ gridTemplateColumns: "minmax(0,1fr) auto" });
export const bubble = style({
  justifySelf: "start",
  maxWidth: "100%",
  padding: "9px 16px",
  borderRadius: "12px",
  background: "#ffffff14",
  color: shellTokens.ink,
  font: `19px/1.35 ${shellTokens.serif}`,
  whiteSpace: "pre-wrap",
  overflowWrap: "anywhere",
  selectors: {
    [`${mine} &`]: {
      justifySelf: "end",
      background: "linear-gradient(180deg,#8a7656,#6d5c43)",
      color: "#fbf3e3",
    },
  },
});
export const time = style({
  paddingTop: "10px",
  font: `16px/1 ${shellTokens.serif}`,
  color: "#b7ad9c",
  whiteSpace: "nowrap",
});
export const card = style({
  margin: "0 0 14px 54px",
  padding: "12px 14px 14px 16px",
  border: `1px solid #ffffff1f`,
  borderRadius: "12px",
  background: "#ffffff08",
});
export const cardHead = style({
  display: "flex",
  alignItems: "center",
  gap: "12px",
  font: `21px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
globalStyle(`${cardHead} svg`, {
  width: "26px",
  height: "26px",
  color: champagne,
});
export const cardDetail = style({
  marginLeft: "auto",
  font: `16px/1 ${shellTokens.serif}`,
  color: champagne,
});
export const cardActions = style({
  display: "flex",
  justifyContent: "flex-end",
  gap: "10px",
  marginTop: "10px",
});
globalStyle(`${cardActions} > button`, { minWidth: "108px" });
export const cardOutcome = style({
  marginTop: "8px",
  font: `italic 18px/1.2 ${shellTokens.serif}`,
  color: "#cfc4b1",
  textAlign: "right",
});
export const requestCard = style([card, { marginLeft: 0 }]);
export const empty = style({
  display: "grid",
  placeItems: "center",
  height: "100%",
  font: `italic 24px/1.3 ${shellTokens.serif}`,
  color: shellTokens.ink,
  textAlign: "center",
});
export const status = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "12px",
  margin: "0 16px 6px 18px",
  font: `18px/1.25 ${shellTokens.serif}`,
  color: shellTokens.ink,
});
globalStyle(`${status} > button`, { flexShrink: 0, whiteSpace: "nowrap" });
export const blocked = style([
  status,
  {
    justifyContent: "center",
    padding: "8px 12px",
    borderRadius: "8px",
    background: "#ffffff10",
  },
]);
globalStyle(`${blocked} svg`, { width: "18px", height: "18px" });

export const composer = style({
  display: "flex",
  alignItems: "flex-end",
  gap: "8px",
  margin: "0 16px 16px 18px",
  padding: "6px 8px 6px 16px",
  border: `1px solid #ffffff26`,
  borderRadius: "10px",
  background: "#ffffff0a",
  selectors: { "&:focus-within": { borderColor: champagne } },
});
export const textarea = style({
  flex: 1,
  minWidth: 0,
  maxHeight: "140px",
  minHeight: "34px",
  padding: "6px 0",
  border: 0,
  background: "transparent",
  color: shellTokens.ink,
  font: `19px/1.35 ${shellTokens.serif}`,
  resize: "none",
  outline: "none",
  selectors: {
    "&::placeholder": { color: "#b7ad9c" },
    "&:disabled": { opacity: 0.55 },
  },
});

/** Requests and Find People rows: picture, name and a line, opening the thread. */
export const list = style({
  display: "flex",
  flexDirection: "column",
  gap: "10px",
  margin: 0,
  padding: "14px 16px",
  listStyle: "none",
  overflowY: "auto",
  minHeight: 0,
  flex: 1,
});
export const row = style({
  appearance: "none",
  display: "grid",
  gridTemplateColumns: "64px minmax(0,1fr) auto",
  alignItems: "center",
  gap: "0 14px",
  width: "100%",
  padding: "8px 14px 8px 12px",
  border: "1px solid #ffffff17",
  borderRadius: "10px",
  background: "#ffffff07",
  color: shellTokens.ink,
  textAlign: "left",
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#ffffff0f" },
    "&:focus-visible": focus,
  },
});
export const rowName = style({
  font: `22px/1.2 ${shellTokens.serif}`,
  overflowWrap: "anywhere",
});
export const rowLine = style({
  display: "block",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  font: `italic 18px/1.3 ${shellTokens.serif}`,
  color: "#e8dcc6",
});
export const rowTrail = style({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  color: shellTokens.ink,
});
globalStyle(`${rowTrail} svg`, { width: "20px", height: "20px" });
export const dot = style({
  width: "10px",
  height: "10px",
  borderRadius: "50%",
  background: "#f0c873",
});
export const search = style({ padding: "14px 16px 0" });
export const quiet = style({
  margin: 0,
  padding: "18px 16px",
  font: `italic 20px/1.3 ${shellTokens.serif}`,
  color: "#cfc4b1",
  textAlign: "center",
});
const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  display: "inline-block",
  width: "24px",
  height: "24px",
  borderRadius: "50%",
  border: "3px solid #ffffff2b",
  borderTopColor: "#efe2c9",
  "@media": { [standardMotion]: { animation: `${spin} 1s linear infinite` } },
});
export const notice = style({ padding: "0 16px 12px" });
