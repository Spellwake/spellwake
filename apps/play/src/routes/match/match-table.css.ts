import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * The live Three.js 3D match table (NP-01 to NP-03) and its overlays.
 * All felt, cards, sleeves, instanced deck stacks, shadows, and status rims
 * render on the WebGL2 canvas; transparent hit/anchor boxes align interactive
 * controls and SVG overlays to the projected 3D cards.
 */
const ivory = "#f4ead2";
const gold = "#e7cf9f";
const copper = "#ee9a63";
const teal = "#7fe9e0";

export const stage = style({
  position: "absolute",
  inset: 0,
  // Clipped, not hidden: focusing a held card must never scroll the table.
  overflow: "clip",
  background: "#283329",
  userSelect: "none",
});
export const canvas = style({
  position: "absolute",
  inset: 0,
  display: "block",
  width: "100%",
  height: "100%",
  touchAction: "none",
});
export const hitLayer = style({
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
});
export const hitAnchor = style({
  position: "absolute",
  pointerEvents: "auto",
});
export const hitButton = style({
  appearance: "none",
  display: "block",
  position: "relative",
  width: "100%",
  height: "100%",
  padding: 0,
  border: 0,
  borderRadius: "6PX",
  background: "transparent",
  color: "inherit",
  font: "inherit",
  cursor: "pointer",
  selectors: {
    "&:focus-visible": {
      outline: `3PX solid ${t.color.focus}`,
      outlineOffset: "3PX",
    },
    "&:disabled": { cursor: "default" },
  },
});

export const badge = style({
  position: "absolute",
  right: "-4PX",
  bottom: "-4PX",
  minWidth: "24PX",
  padding: "3PX 6PX",
  borderRadius: "999PX",
  border: `1.5PX solid ${gold}`,
  background: "#0c1016f2",
  color: ivory,
  font: `600 12PX/1 ${shellTokens.serif}`,
  textAlign: "center",
  pointerEvents: "none",
  whiteSpace: "nowrap",
});
export const countBadge = style([
  badge,
  { right: "auto", left: "-4PX", bottom: "-4PX" },
]);

export const seatLabel = style({
  position: "absolute",
  transform: "translate(-50%, -50%)",
  font: `20PX/1.2 ${shellTokens.serif}`,
  letterSpacing: "0.02em",
  color: ivory,
  textShadow: "0 1PX 6PX #000a",
  pointerEvents: "none",
  whiteSpace: "nowrap",
});
export const srOnly = style({
  position: "absolute",
  width: "1PX",
  height: "1PX",
  overflow: "hidden",
  clip: "rect(0 0 0 0)",
  whiteSpace: "nowrap",
});

/** Declaration lines, over the table and under the floating controls. */
export const lines = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  overflow: "visible",
  pointerEvents: "none",
});
export const line = style({
  fill: "none",
  strokeWidth: "2.2PX",
  strokeLinecap: "round",
  filter: "drop-shadow(0 1PX 3PX #000a)",
  selectors: {
    '&[data-tone="copper"]': { stroke: copper },
    '&[data-tone="ivory"]': { stroke: ivory },
  },
});
export const lineHead = style({
  selectors: {
    '&[data-tone="copper"]': { fill: copper },
    '&[data-tone="ivory"]': { fill: ivory },
  },
});
/** A player offered as a defender, or chosen as one. */
export const seatAnchor = style({
  position: "relative",
  display: "inline-flex",
  borderRadius: "999PX",
  selectors: {
    '&[data-recipient="offered"]': {
      boxShadow: `0 0 0 2PX ${copper}cc`,
    },
    '&[data-recipient="assigned"]': {
      boxShadow: `0 0 0 3PX ${copper}, 0 0 18PX ${copper}99`,
    },
    // A player who may be dealt the damage being assigned, or a spell's target.
    // A player who is out of the game stays in their seat, muted.
    "&[data-out]": { opacity: 0.58 },
    '&[data-recipient="marked"]': {
      boxShadow: `0 0 0 2PX ${ivory}, 0 0 14PX ${ivory}66`,
    },
  },
});

/** Damage controls, beside the cards and portraits they assign to (DA-01 to DA-08). */
const coarse = "(pointer: coarse)";
export const pillLayer = style({
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
});
export const leader = style({
  fill: "none",
  stroke: `${ivory}d9`,
  strokeWidth: "1.5PX",
  strokeLinecap: "round",
});
export const pill = style({
  position: "absolute",
  left: 0,
  top: 0,
  display: "grid",
  gridTemplateColumns: "var(--pill-step) minmax(0, 1fr) var(--pill-step)",
  alignItems: "center",
  width: "112PX",
  height: "38PX",
  boxSizing: "border-box",
  borderRadius: "999PX",
  border: `1.5PX solid ${ivory}`,
  background: "#232a22f2",
  boxShadow: "0 6PX 16PX #0009",
  color: ivory,
  pointerEvents: "auto",
  vars: { "--pill-step": "36PX" },
  "@media": {
    [coarse]: {
      width: "136PX",
      // Its buttons are 44 high inside the border.
      height: "47PX",
      vars: { "--pill-step": "44PX" },
    },
  },
});
export const pillStep = style({
  appearance: "none",
  display: "grid",
  placeItems: "center",
  width: "100%",
  height: "100%",
  padding: 0,
  border: 0,
  borderRadius: "999PX",
  background: "none",
  color: "inherit",
  font: `400 22PX/1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&:disabled": { opacity: 0.34, cursor: "default" },
    "&:focus-visible": {
      outline: `2PX solid ${ivory}`,
      outlineOffset: "-4PX",
    },
    "&:not(:disabled):hover": { background: "#ffffff14" },
  },
});
export const pillAmount = style({
  display: "block",
  textAlign: "center",
  font: `400 24PX/1 ${shellTokens.serif}`,
  fontVariantNumeric: "tabular-nums",
});
/** The player's name above a pill that hangs from their portrait. */
export const pillCaption = style({
  position: "absolute",
  left: "50%",
  bottom: "calc(100% + 4PX)",
  transform: "translateX(-50%)",
  font: `14PX/1 ${shellTokens.serif}`,
  color: ivory,
  textShadow: "0 1PX 4PX #000c",
  whiteSpace: "nowrap",
  pointerEvents: "none",
});

/** The spell pile, to the right of the table and facing the camera (CP-01 to CP-10). */
export const pileHost = style({
  position: "absolute",
  left: 0,
  top: 0,
  transformOrigin: "0 0",
});

/**
 * While a spell waits for its target, a player is chosen by their portrait:
 * this button covers the picture, and the rest of the chip still moves the
 * camera.
 */
export const seatTarget = style({
  appearance: "none",
  position: "absolute",
  left: 0,
  top: 0,
  bottom: 0,
  aspectRatio: "1",
  minWidth: "44PX",
  padding: 0,
  border: 0,
  borderRadius: "999PX",
  background: `${teal}22`,
  boxShadow: `inset 0 0 0 2PX ${teal}`,
  cursor: "pointer",
  selectors: {
    "&:hover": { background: `${teal}44` },
    "&:focus-visible": {
      outline: `3PX solid ${ivory}`,
      outlineOffset: "2PX",
    },
  },
});

/** What a cast waits for, named over the table (BC-02, BC-04). */
export const castPrompt = style({
  position: "absolute",
  left: 0,
  right: 0,
  top: "24%",
  display: "flex",
  justifyContent: "center",
  pointerEvents: "none",
  color: ivory,
  font: `26PX/1.2 ${shellTokens.serif}`,
  letterSpacing: "0.02em",
  textShadow: "0 1PX 6PX #000c",
  whiteSpace: "nowrap",
  selectors: {
    // Upright on a phone the pending spell stands at the upper right, so the
    // prompt sits beneath it.
    ':root[data-shell="compact"][data-orientation="portrait"] &': {
      top: "46%",
      fontSize: "22PX",
    },
    // A divided amount's pills stand above the cards there, so its words
    // sit higher, between the spell and the pills.
    ':root[data-shell="compact"][data-orientation="portrait"] &[data-raised]': {
      top: "37%",
    },
  },
  "@media": {
    // On a phone held sideways the pills leave no clear place over the
    // table: the words stand with the actions instead.
    "(max-height: 480px)": {
      selectors: { "&[data-raised]": { display: "none" } },
    },
  },
});
globalStyle(`${castPrompt} p`, { margin: 0 });

/** Choosing a mode on the original card (MC-01, MC-02): over the dimmed table. */
export const modeDialog = style({
  width: "100vw",
  height: "100dvh",
  maxWidth: "none",
  maxHeight: "none",
  margin: 0,
  padding: 0,
  border: 0,
  background: "#0b100ccc",
  color: ivory,
  overflow: "hidden",
  selectors: { "&::backdrop": { background: "transparent" } },
});
export const modeScreen = style({
  height: "100%",
  boxSizing: "border-box",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: "22PX",
  padding: "18PX 20PX 28PX",
  "@media": {
    // A phone on its side: everything draws closer so Cancel stays in view.
    "(max-height: 480px)": { gap: "8PX", padding: "8PX 16PX 10PX" },
  },
});
export const modeTop = style({
  alignSelf: "stretch",
  display: "flex",
  justifyContent: "flex-end",
});
export const modePeek = style({
  appearance: "none",
  minHeight: "44PX",
  padding: "6PX 24PX",
  borderRadius: "999PX",
  border: `1.5PX solid ${ivory}`,
  background: "#161c17e6",
  color: ivory,
  font: `22PX/1.1 ${shellTokens.serif}`,
  cursor: "pointer",
  selectors: {
    "&:hover": { background: "#232b24" },
    "&:focus-visible": {
      outline: `3PX solid ${t.color.focus}`,
      outlineOffset: "3PX",
    },
  },
});
export const modeHeading = style({
  margin: 0,
  font: `600 44PX/1.1 ${shellTokens.serif}`,
  letterSpacing: "0.01em",
  textShadow: "0 2PX 8PX #000c",
  "@media": {
    // Beside View Battlefield, and clear of it: the faces start beneath both.
    "(max-height: 480px)": { fontSize: "26PX", marginTop: "-40PX" },
  },
});
/**
 * The faces. Two rows on a small phone are taller than the screen, so they
 * scroll here while View Battlefield, Cancel and Next stay in view.
 */
export const modeFaces = style({
  flex: "0 1 auto",
  alignSelf: "stretch",
  minHeight: 0,
  display: "flex",
  justifyContent: "center",
  overflowY: "auto",
  overscrollBehavior: "contain",
  // Room for a face's shadow inside the scrolling edge.
  padding: "4PX 0 6PX",
  margin: "-4PX 0 -6PX",
});
export const modeFooter = style({
  display: "flex",
  justifyContent: "center",
  gap: "16PX",
  minWidth: "min(270PX, 80vw)",
});
globalStyle(`${modeFooter} > button`, { flex: 1 });
/** Next, once enough modes are marked: the one gold action (MM-03). */
export const modeNext = style({});
globalStyle(`${modeFooter} > button${modeNext}:enabled`, {
  borderColor: "#fff3c9",
  background: "linear-gradient(180deg, #f6e7b4, #d8c07a)",
  color: "#1d1a10",
  textShadow: "none",
});
globalStyle(`${modeFooter} > button${modeNext}:enabled:hover`, {
  background: "linear-gradient(180deg, #fdf1c6, #e3cc88)",
});

/**
 * The spell being cast on its own face, beside the table, and the lines from
 * its sentences to what was chosen for them (MM-06, MT-02). Nothing here is
 * pressed: the cards, portraits and pile take every choice.
 */
export const sourceLayer = style({
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
});
export const sourceFace = style({
  position: "absolute",
  left: 0,
  top: 0,
  filter: "drop-shadow(0 6PX 14PX #000a)",
});
export const sourceLines = style({
  position: "absolute",
  inset: 0,
  width: "100%",
  height: "100%",
  overflow: "visible",
  fill: ivory,
  filter: "drop-shadow(0 1PX 2PX #000a)",
});
globalStyle(`${sourceLines} path`, {
  fill: "none",
  stroke: ivory,
  strokeWidth: 1.75,
  strokeLinecap: "round",
});
export const sourceLineEnd = style({
  fill: "#1a211b",
  stroke: ivory,
  strokeWidth: 2,
});
