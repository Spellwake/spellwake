import {
  globalFontFace,
  globalStyle,
  keyframes,
  style,
} from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { frost, touch, TOUCH_TARGET } from "../../components/shell/glass.css";

/**
 * Account entry, following `account-entry-desktop-approved-v1`: one compact
 * card over the backdrop, a modest serif heading, sans form type and quiet
 * controls. The card keeps its approved near-solid fill so its text holds over
 * any artwork, and frosts what little shows through.
 */
const sans = '"Albert Sans", system-ui, sans-serif';
const heading = "Lora, Georgia, serif";
// The app is served from "/" natively and "/play/" on the web, so the address carries the base.
const fontUrl = (file: string): string =>
  `url("${import.meta.env.BASE_URL}fonts/${file}") format("woff2")`;
globalFontFace("Albert Sans", {
  src: fontUrl("albert-sans.woff2"),
  fontDisplay: "swap",
});
globalFontFace("Lora", {
  src: fontUrl("lora.woff2"),
  fontDisplay: "swap",
});
const focus = { outline: `2px solid ${t.color.focus}`, outlineOffset: "3px" };

export const scene = style({
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  display: "grid",
  // "safe" keeps the top of a card taller than the screen reachable by scrolling.
  placeItems: "safe center",
  padding:
    "max(24px, env(safe-area-inset-top, 0px)) max(16px, env(safe-area-inset-right, 0px)) max(24px, env(safe-area-inset-bottom, 0px)) max(16px, env(safe-area-inset-left, 0px))",
});
export const stack = style({
  width: "min(490px, 100%)",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "20px",
});
export const card = style({
  // Shared controls inside the card take the card's sans type and its smaller scale.
  vars: {
    [t.font.face]: sans,
    [t.font.size]: "18px",
    [t.font.small]: "17px",
    [t.font.hint]: "16px",
    [t.size.control]: "48px",
  },
  boxSizing: "border-box",
  width: "100%",
  padding: "29px 45px 40px",
  border: "1px solid #6370789c",
  borderRadius: "34px",
  background: "linear-gradient(135deg, #1a2938ea, #101d29f0)",
  backdropFilter: `blur(${frost})`,
  boxShadow: "0 12px 48px #0005",
  color: "#f3e7cd",
  font: `18px/1.4 ${sans}`,
  textAlign: "center",
  WebkitFontSmoothing: "antialiased",
  "@media": {
    // Fields keep to a fingertip where the screen is touched.
    [touch]: { vars: { [t.size.control]: `max(48px, ${TOUCH_TARGET})` } },
    "(max-width: 430PX)": { padding: "26px 28px 32px", borderRadius: "28px" },
    "(forced-colors: active)": {
      background: "Canvas",
      borderColor: "CanvasText",
    },
  },
});
export const mark = style({
  display: "block",
  width: "40px",
  height: "44px",
  margin: "0 auto",
});
export const title = style({
  margin: "14px 0 24px",
  font: `26px/1.28 ${heading}`,
  textWrap: "balance",
  selectors: { "&:focus-visible": { outline: "none" } },
});
/** A heading with a line of context under it sits closer to that line. */
export const titleWithLede = style({ marginBottom: "10px" });
export const lede = style({ margin: "0 0 24px", overflowWrap: "anywhere" });
export const or = style({
  display: "flex",
  alignItems: "center",
  gap: "28px",
  padding: "17px 0",
  color: "#e4d9c8",
  selectors: {
    "&::before, &::after": {
      content: '""',
      flex: 1,
      height: "1px",
      background: "#6370789c",
    },
  },
});
export const form = style({ display: "grid", gap: "16px", textAlign: "left" });
/**
 * A refused field reads red in this card, as approved in AE-05 and as the code
 * slots already do. The shared field's own error colour is gold.
 */
export const field = style({});
globalStyle(`${field} div:has(> input[aria-invalid="true"])`, {
  borderColor: "#ff795f",
  boxShadow: "inset 0 0 0 .5px #ff8978, 0 0 9px #ff755f5c",
  outlineColor: "#ff795f",
});
globalStyle(`${field}:has(input[aria-invalid="true"]) > :last-child`, {
  color: "#ff7767",
});
/** The approved cream primary action, held to the card's width. */
export const submit = style({
  selectors: {
    "&&": {
      width: "100%",
      minHeight: "52px",
      border: 0,
      borderRadius: "12px",
      background: "#edddbd",
      color: "#1e1912",
      font: `600 18px/1.1 ${sans}`,
      textShadow: "none",
      boxShadow: "none",
    },
    "&&:enabled:hover": { background: "#f6e8cb" },
    "&&:disabled": { background: "#8d8676", color: "#2c2a25", opacity: 1 },
    "&&:focus-visible": focus,
  },
  "@media": { [touch]: { selectors: { "&&": { minHeight: TOUCH_TARGET } } } },
});
/** A text action outside or under the card: switch mode, change email. */
export const link = style({
  appearance: "none",
  display: "inline-flex",
  alignItems: "center",
  gap: "8px",
  // 32 design pixels, but never below a 24 px pointer target at small scales.
  minHeight: "max(32px, 24PX)",
  padding: "5px 6px",
  border: 0,
  borderRadius: "6px",
  background: "transparent",
  color: "#f3e7cd",
  font: `17px/1.2 ${sans}`,
  textDecoration: "none",
  textShadow: "0 1px 3px #05090f",
  cursor: "pointer",
  selectors: {
    "&:focus-visible": focus,
    "&:disabled": { opacity: 0.5, cursor: "default" },
  },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});
export const linkAccent = style({ color: "#ffd98f", fontWeight: 600 });
const spin = keyframes({ to: { rotate: "1turn" } });
/** A passive status line at the foot of the card. It never carries a control. */
export const notice = style({
  display: "flex",
  alignItems: "center",
  gap: "12px",
  margin: "20px 0 0",
  padding: "9px 16px",
  border: "1px solid #3d4a57",
  borderRadius: "10px",
  background: "#16222dcc",
  color: "#f3e5c4",
  font: `16px/1.3 ${sans}`,
  textAlign: "left",
});
export const noticeIcon = style({
  flexShrink: 0,
  width: "20px",
  height: "20px",
});
export const spinner = style({
  "@media": { [standardMotion]: { animation: `${spin} 0.9s linear infinite` } },
});
/** The large ring on the checking card. */
export const bigSpinner = style({
  display: "block",
  width: "36px",
  height: "36px",
  margin: "22px auto 4px",
  color: "#c9bfa8",
  "@media": { [standardMotion]: { animation: `${spin} 0.9s linear infinite` } },
});
