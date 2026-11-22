import { globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * The trade room, as approved (trade-approval-v1, TR-D01 to TR-D11): Back and
 * the title, two equal trays, then the shared status and actions. Lengths are
 * in the pixel scale of the approved references. Narrower than `STACKED` the
 * trays stack, your offer first, and the page scrolls instead of the trays.
 */
const STACKED = "trade (inline-size < 60rem)";
const phone = ':root[data-device="phone"] &';
const compact = ':root[data-shell="compact"] &';

export const page = style({
  containerName: "trade",
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  overflowY: "auto",
  overscrollBehavior: "contain",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  padding: "26px 4.2% 34px",
  selectors: {
    // The compact shell's dock floats over the bottom; the last actions clear it.
    [compact]: { paddingBottom: "130px" },
    [phone]: { padding: "14px 16px 150px" },
  },
});

export const head = style({
  display: "flex",
  alignItems: "center",
  gap: "40px",
  minHeight: "88px",
  marginBottom: "26px",
  "@container": { [STACKED]: { gap: "20px", marginBottom: "18px" } },
});
export const back = style({ flex: "none" });
export const title = style({
  margin: 0,
  minWidth: 0,
  font: `44px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  textShadow: "0 2px 8px #000c",
  overflowWrap: "anywhere",
  selectors: { [phone]: { fontSize: "34px" } },
});

/**
 * The actions follow both offers on every screen, never pinned over them: on
 * a phone you scroll past what the other person gives before you commit.
 */
/** Two equal trays that share the height the shell leaves; their lists scroll. */
export const columns = style({
  flex: "1 1 auto",
  minHeight: "420px",
  maxHeight: "640px",
  display: "grid",
  gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
  gap: "16px",
  "@container": {
    [STACKED]: {
      flex: "none",
      maxHeight: "none",
      minHeight: 0,
      gridTemplateColumns: "minmax(0,1fr)",
      gap: "18px",
    },
  },
});
export const slot = style({
  minWidth: 0,
  minHeight: 0,
  height: "100%",
  "@container": { [STACKED]: { height: "auto" } },
});
/** Stacked, a tray grows with its rows: it contains only its width, so its height is its content's. */
globalStyle(`${slot} > section`, {
  "@container": { [STACKED]: { containerType: "inline-size", height: "auto" } },
});

export const footer = style({
  display: "grid",
  // The actions keep their approved width; the status line takes the rest.
  gridTemplateColumns: "minmax(0,1fr) minmax(0,552px)",
  alignItems: "center",
  gap: "6%",
  paddingTop: "40px",
  paddingRight: "0.4%",
  "@container": {
    [STACKED]: {
      gridTemplateColumns: "minmax(0,1fr)",
      gap: "18px",
      paddingTop: "26px",
    },
  },
});
export const status = style({ minWidth: 0 });
globalStyle(`${footer} button`, { whiteSpace: "nowrap" });
globalStyle(`${footer} [role="group"]`, {
  "@container": { [STACKED]: { justifySelf: "center" } },
});

const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  flex: "none",
  display: "inline-block",
  width: "34px",
  height: "34px",
  borderRadius: "50%",
  border: "3px solid #ffffff2b",
  borderTopColor: "#efe2c9",
  "@media": { [standardMotion]: { animation: `${spin} 1s linear infinite` } },
});
export const opening = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "18px",
  marginTop: "18vh",
  font: `italic 26px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
});

/** An unconfirmed Trade: the uncertainty, and Check status beside it. */
export const checking = style({
  display: "grid",
  gridTemplateColumns: "auto minmax(0,1fr) auto",
  alignItems: "center",
  gap: "8px 22px",
  "@container": {
    [STACKED]: { gridTemplateColumns: "auto minmax(0,1fr)" },
  },
});
export const checkingCopy = style({ minWidth: 0 });
export const checkingTitle = style({
  margin: 0,
  font: `36px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
});
export const checkingText = style({
  margin: "4px 0 0",
  font: `24px/1.3 ${shellTokens.serif}`,
  color: "#e6dccb",
});
export const check = style({
  minWidth: "220px",
  "@container": { [STACKED]: { gridColumn: "1 / -1", justifySelf: "center" } },
});

export const notices = style({ position: "relative", height: 0 });

export const receipt = style({ flex: "1 1 auto", minHeight: 0 });
export const closed = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  padding: "40px 0",
});

/** Card artwork in a landscape frame, as the approved rows show it. */
export const card = style({
  display: "block",
  width: "100%",
  aspectRatio: "1.55",
  overflow: "hidden",
  borderRadius: "6px",
  border: "1px solid #c9b99b8c",
  background: "linear-gradient(160deg,#1a2028,#0b0f14)",
  boxShadow: "0 3px 10px #000a",
});
export const cardArt = style({
  display: "block",
  width: "100%",
  height: "100%",
  objectFit: "cover",
});
/** The drawn pack wrapper, at the approved rows' size. */
export const pack = style({
  display: "block",
  width: "min(100%, 118px)",
  marginInline: "auto",
});
export const coin = style({
  display: "block",
  width: "min(70%, 68px)",
  height: "auto",
  marginInline: "auto",
  filter: "drop-shadow(0 3px 6px #000a)",
});
/** A portrait cropped into the tray's round avatar. */
export const face = style({
  position: "relative",
  display: "block",
  width: "100%",
  height: "100%",
  overflow: "hidden",
  borderRadius: "50%",
});
export const blank = style({
  width: "100%",
  height: "100%",
  color: t.color.muted,
});
/** An editable row keeps the card rows' height, so its pack is smaller. */
globalStyle(`[data-editable] ${pack}`, { width: "min(100%, 66px)" });
