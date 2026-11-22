import { createVar, globalStyle, keyframes, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { TOUCH_TARGET, touch } from "../../components/shell/glass.css";
import { shellTokens } from "../../components/shell/shell.css";
import { cssEase, motionLanguage as m } from "../../lib/motion";

/**
 * Collection Packs, as approved (CP-01 to CP-13): the owned pack library and
 * the borderless opening stage. Lengths are in the pixel scale of the
 * approved references.
 */
const phone = ':root[data-device="phone"] &';
const phoneLandscape =
  ':root[data-device="phone"][data-orientation="landscape"] &';
const arrive = cssEase(m.ease.arrive);
const focus = {
  outline: `2px solid ${t.color.focus}`,
  outlineOffset: "3px",
} as const;
const TOOLBAR_STACKED = "packs (inline-size < 40rem)";

/** A wrapper's colours, set per pack. */
export const hue = createVar();
export const deep = createVar();
export const glow = createVar();

export const page = style({
  containerName: "packs",
  containerType: "inline-size",
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  padding: "36px 3.3% 0 52px",
  selectors: { [phone]: { padding: "22px 22px 0" } },
});
export const toolbar = style({
  flexShrink: 0,
  display: "grid",
  gridTemplateColumns: "minmax(0,432px) minmax(0,268px)",
  gap: "36px",
  "@container": {
    [TOOLBAR_STACKED]: {
      gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
      gap: "14px",
    },
  },
});
globalStyle(`${toolbar} > *`, { minWidth: 0 });
globalStyle(`${toolbar} > button`, { width: "100%" });
export const search = style({
  "@container": { [TOOLBAR_STACKED]: { gridColumn: "1 / -1" } },
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
    [phone]: {
      marginInline: "-22px",
      paddingInline: "22px",
      scrollPaddingBottom: "140px",
    },
  },
});
export const grid = style({
  display: "grid",
  gridTemplateColumns: "repeat(auto-fill, 280px)",
  justifyContent: "start",
  gap: "36px 30px",
  padding: "44px 0 56px",
  margin: 0,
  listStyle: "none",
  selectors: {
    [phone]: {
      gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
      gap: "32px 18px",
      padding: "28px 0 150px",
    },
    [phoneLandscape]: {
      gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))",
      padding: "22px 0 40px",
    },
  },
});
export const tile = style({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  textAlign: "center",
});
export const wrapperSlot = style({
  width: "238px",
  maxWidth: "86%",
  selectors: { [phone]: { maxWidth: "92%" } },
});
export const setName = style({
  margin: "24px 0 0",
  font: `29px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  color: shellTokens.ink,
  selectors: { [phone]: { fontSize: "25px", marginTop: "16px" } },
});
export const owned = style({
  margin: "6px 0 0",
  font: `24px/1.2 ${shellTokens.serif}`,
  color: "#e8dcc6",
});
export const openButton = style({ marginTop: "18px" });
globalStyle(`${openButton} > button`, { minWidth: "194px" });

/** A pack on the shelf: the 3D still, or the drawn wrapper until it is ready. */
export const packArt = style({
  position: "relative",
  display: "block",
  width: "100%",
  aspectRatio: "238 / 420",
});
const stillArrive = keyframes({ from: { opacity: 0 }, to: { opacity: 1 } });
export const still = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  filter: "drop-shadow(0 18px 20px #000b)",
  "@media": {
    [standardMotion]: {
      animation: `${stillArrive} ${m.duration.scene}s ${arrive} both`,
    },
  },
});
/** Light crossing the foil on hover or focus, inside the pinked crimps. */
export const stillSheen = style({
  position: "absolute",
  inset: "0.8% 0",
  overflow: "hidden",
  pointerEvents: "none",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      inset: 0,
      background:
        "linear-gradient(100deg, transparent 30%, #ffffff38 46%, #ffffff10 54%, transparent 66%)",
      mixBlendMode: "screen",
      transform: "translateX(-120%) skewX(-16deg)",
    },
  },
});
/** A sealed pack, drawn in code: a foil wrapper with crimped ends, the set symbol and the label. */
export const wrapper = style({
  position: "relative",
  display: "block",
  width: "100%",
  aspectRatio: "238 / 420",
  borderRadius: "6px",
  overflow: "hidden",
  isolation: "isolate",
  containerType: "inline-size",
  background: [
    `radial-gradient(70% 34% at 50% 38%, ${glow}, transparent 70%)`,
    `radial-gradient(120% 60% at 20% 100%, ${deep}, transparent 70%)`,
    `linear-gradient(172deg, ${hue}, ${deep} 72%)`,
  ].join(","),
  boxShadow:
    "0 18px 34px #000c, inset 0 0 0 1px #ffffff26, inset 0 0 22px #00000080",
  selectors: {
    // Crimped seals at both ends.
    "&::before, &::after": {
      content: '""',
      position: "absolute",
      left: 0,
      right: 0,
      height: "5.4%",
      background: `repeating-linear-gradient(90deg, #ffffff40 0 2px, #00000050 2px 5px), linear-gradient(180deg, ${hue}, ${deep})`,
      zIndex: 2,
    },
    "&::before": { top: 0 },
    "&::after": { bottom: 0 },
  },
});
/** Light running across the foil. It moves only where motion is welcome. */
const sheenSweep = keyframes({
  "0%": { transform: "translateX(-120%) skewX(-16deg)" },
  "100%": { transform: "translateX(220%) skewX(-16deg)" },
});
export const sheen = style({
  position: "absolute",
  inset: 0,
  zIndex: 1,
  background:
    "linear-gradient(100deg, transparent 30%, #ffffff2e 46%, #ffffff0d 54%, transparent 66%)",
  mixBlendMode: "screen",
  pointerEvents: "none",
  transform: "translateX(-120%) skewX(-16deg)",
  "@media": {
    [standardMotion]: {
      selectors: {
        [`${wrapper}:hover &, ${tile}:focus-within &`]: {
          animation: `${sheenSweep} 1.6s ${arrive} 1`,
        },
      },
    },
  },
});
globalStyle(
  `${tile}:hover ${stillSheen}::after, ${tile}:focus-within ${stillSheen}::after`,
  {
    "@media": {
      [standardMotion]: { animation: `${sheenSweep} 1.6s ${arrive} 1` },
    },
  },
);
export const emblem = style({
  position: "absolute",
  left: "50%",
  top: "39.5%",
  width: "120%",
  aspectRatio: "1",
  transform: "translate(-50%, -50%)",
  borderRadius: "50%",
  // A sunburst of soft rays around the set symbol, as on the 3D print.
  background: [
    `radial-gradient(circle at 46% 44%, #ffffff40, ${glow} 22%, transparent 34%)`,
    `repeating-conic-gradient(from -90deg, #ffffff14 0 9deg, transparent 15deg 30deg)`,
  ].join(","),
  WebkitMaskImage: "radial-gradient(closest-side, #000 18%, transparent)",
  maskImage: "radial-gradient(closest-side, #000 18%, transparent)",
});
/** The set's official symbol, inked in gold through its own shape. */
export const symbol = style({
  position: "absolute",
  left: "50%",
  top: "39.5%",
  zIndex: 2,
  width: "44%",
  aspectRatio: "1",
  transform: "translate(-50%, -50%)",
  background: "linear-gradient(180deg, #fbecc4, #f2dba6 55%, #c39a52)",
  maskSize: "contain",
  WebkitMaskSize: "contain",
  maskRepeat: "no-repeat",
  WebkitMaskRepeat: "no-repeat",
  maskPosition: "center",
  WebkitMaskPosition: "center",
});
export const label = style({
  position: "absolute",
  left: "6%",
  right: "6%",
  bottom: "12%",
  zIndex: 2,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "10px",
  color: "#fbf3e4",
  textAlign: "center",
  textShadow: "0 2px 6px #000c",
});
/** The set's name, sized to the wrapper so a long word shrinks instead of breaking. */
export const labelSet = style({
  font: `9.5cqw/1.02 ${shellTokens.serif}`,
  textTransform: "uppercase",
  letterSpacing: "0.02em",
  textWrap: "balance",
});
export const labelRule = style({
  width: "74%",
  height: "1px",
  background: "linear-gradient(90deg, transparent, #f6e2b6, transparent)",
  position: "relative",
  selectors: {
    "&::after": {
      content: '""',
      position: "absolute",
      left: "50%",
      top: "50%",
      width: "6px",
      height: "6px",
      background: "#f6e2b6",
      transform: "translate(-50%, -50%) rotate(45deg)",
    },
  },
});
export const labelKind = style({
  font: `13px/1 ${shellTokens.serif}`,
  letterSpacing: "0.24em",
  textTransform: "uppercase",
});
/** A pack still loading: the wrapper's shape, dark and plain. */
export const blank = style({
  display: "block",
  width: "100%",
  aspectRatio: "238 / 420",
  borderRadius: "6px",
  background:
    "linear-gradient(170deg,#262c33,#151a20 70%), repeating-linear-gradient(90deg,#ffffff10 0 2px,transparent 2px 5px)",
  boxShadow: "0 18px 34px #000a, inset 0 0 0 1px #ffffff14",
});

export const center = style({
  flex: 1,
  display: "grid",
  placeItems: "center",
  paddingBottom: "8%",
});
export const quietTitle = style({
  margin: 0,
  font: `italic 34px/1.2 ${shellTokens.serif}`,
  color: shellTokens.ink,
  textAlign: "center",
});
export const textLink = style({
  appearance: "none",
  display: "block",
  margin: "14px auto 0",
  padding: "6px 4px",
  border: 0,
  background: "none",
  color: shellTokens.ink,
  font: `25px/1.2 ${shellTokens.serif}`,
  textDecoration: "underline",
  textUnderlineOffset: "5px",
  cursor: "pointer",
  selectors: { "&:focus-visible": focus },
  "@media": { [touch]: { minHeight: TOUCH_TARGET } },
});

/** The opening stage: borderless over the whole window, the library inert below. */
export const stage = style({
  position: "fixed",
  inset: 0,
  width: "100%",
  height: "100%",
  maxWidth: "none",
  maxHeight: "none",
  margin: 0,
  padding: 0,
  border: 0,
  background: "transparent",
  color: shellTokens.ink,
  overflow: "hidden",
  selectors: {
    "&::backdrop": {
      background:
        "radial-gradient(80% 60% at 50% 40%, #0d1a2bcc, #04070bf2 75%)",
      backdropFilter: "blur(10px)",
    },
  },
});
/** The 3D stage, under the title, status and cards. */
export const stageCanvas = style({
  position: "absolute",
  inset: 0,
  zIndex: 0,
  pointerEvents: "none",
  overflow: "hidden",
});
export const stageInner = style({
  position: "relative",
  zIndex: 1,
  height: "100%",
  display: "flex",
  flexDirection: "column",
  padding: "26px 30px 0",
  selectors: { [phone]: { padding: "12px 12px 0" } },
});
/** Title centred, Close at the end, both outside the scrolling cards. */
export const stageHead = style({
  flexShrink: 0,
  display: "grid",
  gridTemplateColumns: "1fr auto 1fr",
  alignItems: "start",
  gap: "12px",
  paddingTop: "30px",
  selectors: { [phone]: { paddingTop: "6px" } },
});
export const stageClose = style({
  justifySelf: "end",
  marginTop: "-30px",
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "54px",
  height: "54px",
  border: 0,
  borderRadius: "12px",
  background: "#ffffff14",
  color: shellTokens.ink,
  cursor: "pointer",
  selectors: {
    "&:hover:not(:disabled)": { background: "#ffffff24" },
    "&:focus-visible": focus,
    "&:disabled": { opacity: 0.35, cursor: "default" },
    [phone]: { marginTop: 0 },
  },
  "@media": { [touch]: { minWidth: TOUCH_TARGET, minHeight: TOUCH_TARGET } },
});
globalStyle(`${stageClose} svg`, { width: "30px", height: "30px" });
export const stageTitle = style({
  gridColumn: 2,
  margin: 0,
  font: `44px/1.15 ${shellTokens.serif}`,
  fontWeight: 400,
  textAlign: "center",
  textShadow: "0 2px 10px #000",
  selectors: { [phone]: { fontSize: "30px", paddingTop: "6px" } },
});
export const stageBody = style({
  flex: 1,
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  overflowY: "auto",
  padding: "0 4% 40px",
  scrollbarWidth: "thin",
  scrollbarColor: "#71674f transparent",
  selectors: { [phone]: { padding: "0 4px 28px" } },
});
export const status = style({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "22px",
  marginTop: "34px",
  font: `29px/1.2 ${shellTokens.serif}`,
  textAlign: "center",
});
/** With the pack staged, it waits in this space and the status sits below it. */
export const packArea = style({
  flex: 1,
  minHeight: "240px",
  alignSelf: "stretch",
});
export const statusStaged = style([
  status,
  {
    flexShrink: 0,
    marginTop: "8px",
    paddingBottom: "36px",
    font: `25px/1.2 ${shellTokens.serif}`,
    selectors: { [phone]: { fontSize: "21px", paddingBottom: "20px" } },
  },
]);
const spin = keyframes({ to: { transform: "rotate(360deg)" } });
export const spinner = style({
  width: "48px",
  height: "48px",
  borderRadius: "50%",
  border: "3px solid #ffffff2b",
  borderTopColor: "#efe2c9",
  "@media": { [standardMotion]: { animation: `${spin} 1s linear infinite` } },
});

export const cards = style({
  display: "grid",
  gridTemplateColumns: "repeat(7, minmax(0, 184px))",
  justifyContent: "center",
  gap: "22px 16px",
  width: "100%",
  margin: "56px 0 0",
  padding: 0,
  listStyle: "none",
  perspective: "1400px",
  selectors: {
    [phone]: {
      gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
      gap: "14px 10px",
      marginTop: "28px",
    },
    [phoneLandscape]: { gridTemplateColumns: "repeat(7, minmax(0, 1fr))" },
  },
  "@media": {
    "(max-aspect-ratio: 1/1)": {
      gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
    },
  },
});
/** Laid out but unseen while the 3D cards are dealt onto it. */
export const cardsDealing = style([cards, { opacity: 0 }]);
/** One card slot: a button that turns the card over. Revealed, it stays focusable. */
export const slot = style({
  appearance: "none",
  position: "relative",
  display: "block",
  width: "100%",
  aspectRatio: "63 / 88",
  padding: 0,
  border: 0,
  borderRadius: "5%/3.6%",
  background: "none",
  cursor: "pointer",
  transformStyle: "preserve-3d",
  selectors: {
    "&:focus-visible": focus,
    '&[aria-disabled="true"]': { cursor: "default" },
  },
});
export const side = style({
  position: "absolute",
  inset: 0,
  borderRadius: "inherit",
  overflow: "hidden",
  backfaceVisibility: "hidden",
  WebkitBackfaceVisibility: "hidden",
  boxShadow: "0 12px 26px #000b",
});
export const front = style([side, { transform: "rotateY(180deg)" }]);
export const backImage = style({
  display: "block",
  width: "100%",
  height: "100%",
  objectFit: "cover",
});
/** A foil card catches the light once it is face up. */
export const foil = style({
  position: "absolute",
  inset: 0,
  borderRadius: "inherit",
  background:
    "linear-gradient(115deg, #ff7ad933 10%, #7af3ff33 30%, #fff38a33 50%, #9b7aff33 70%, #7affb633 90%)",
  mixBlendMode: "color-dodge",
  pointerEvents: "none",
});
export const actions = style({
  display: "flex",
  flexWrap: "wrap",
  justifyContent: "center",
  gap: "40px",
  marginTop: "auto",
  paddingTop: "48px",
  selectors: { [phone]: { gap: "14px", paddingTop: "28px" } },
});
export const actionsDealing = style([actions, { opacity: 0 }]);
globalStyle(`${actions} > button`, { minWidth: "234px" });
globalStyle(`${actions} > button`, {
  "@media": { [touch]: { minWidth: "150px" } },
});
/** Named for assistive technology only: the section's heading, where focus lands when the last pack goes. */
export const srOnly = style({
  position: "absolute",
  width: "1px",
  height: "1px",
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
});
