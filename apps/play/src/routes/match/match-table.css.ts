import { globalStyle, style } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";
import { standardMotion } from "@spellwake/ui/motion";
import { shellTokens } from "../../components/shell/shell.css";

/**
 * The match table, as approved (NP-01 to NP-03): a felt surface, each seat's
 * permanents in two rows tilted away from the viewer, the hand fanned along
 * the lower edge, and the whole table from above. Sizes here are in real
 * pixels: every card is placed and scaled by the layout model.
 */
const felt =
  "radial-gradient(120% 90% at 50% 38%, #46553f 0%, #3a4737 46%, #2b362c 100%)";
// Woven grain, drawn as an SVG filter so the surface carries no image file.
const grain = `url("data:image/svg+xml;utf8,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="220" height="220"><filter id="n"><feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" seed="7"/><feColorMatrix values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0.09 0"/></filter><rect width="220" height="220" filter="url(#n)"/></svg>',
)}")`;
const ivory = "#f4ead2";
const gold = "#e7cf9f";

export const stage = style({
  position: "absolute",
  inset: 0,
  overflow: "hidden",
  background: `${grain}, ${felt}`,
  userSelect: "none",
});
/** A soft pool of light where the focused board sits. */
export const light = style({
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
  background:
    "radial-gradient(60% 46% at 50% 72%, #ffffff12 0%, transparent 70%), linear-gradient(180deg, #0000003a 0%, transparent 22%, transparent 78%, #00000040 100%)",
});
export const layer = style({
  position: "absolute",
  inset: 0,
  transformOrigin: "50% 100%",
  transformStyle: "preserve-3d",
});
export const handLayer = style({
  position: "absolute",
  inset: 0,
  pointerEvents: "none",
});

/** One card, centred on its placement and scaled from one base size. */
export const BASE_WIDTH = 200;
export const piece = style({
  position: "absolute",
  left: 0,
  top: 0,
  width: `${BASE_WIDTH}PX`,
  aspectRatio: "5 / 7",
  marginLeft: `${-BASE_WIDTH / 2}PX`,
  marginTop: `${(-BASE_WIDTH * 1.4) / 2}PX`,
  transformOrigin: "50% 50%",
  pointerEvents: "auto",
});
export const cardButton = style({
  appearance: "none",
  display: "block",
  position: "relative",
  width: "100%",
  height: "100%",
  padding: 0,
  border: 0,
  borderRadius: "9PX",
  background: "none",
  color: "inherit",
  font: "inherit",
  cursor: "pointer",
  transformOrigin: "50% 100%",
  "@media": {
    [standardMotion]: {
      transition: "transform 220ms cubic-bezier(0.22, 1, 0.36, 1)",
    },
  },
  selectors: {
    "&:focus-visible": {
      outline: `4PX solid ${t.color.focus}`,
      outlineOffset: "4PX",
    },
    // A held card rises out of the fan to be read.
    '&[data-zone="hand"]:hover, &[data-zone="hand"]:focus-visible': {
      transform: "translateY(-34%) scale(1.12)",
    },
    '&[data-zone="battlefield"]:hover': { transform: "translateY(-3%)" },
    "&:disabled": { cursor: "default" },
  },
});
export const face = style({
  display: "block",
  width: "100%",
  height: "100%",
  borderRadius: "9PX",
  objectFit: "cover",
  boxShadow: "0 10PX 22PX #0009, 0 2PX 4PX #000a",
  background: "#16181d",
});
/** Offered for the action in progress: a bright rim, as the approved boards show. */
globalStyle(`${cardButton}[data-offered="true"] ${face}`, {
  boxShadow: `0 0 0 4PX #7fe9e0, 0 0 26PX #7fe9e0aa, 0 10PX 22PX #0009`,
});
globalStyle(`${cardButton}[data-playable="true"] ${face}`, {
  boxShadow: `0 0 0 3PX ${gold}cc, 0 0 22PX ${gold}77, 0 10PX 22PX #0009`,
});

/** A card with no picture: its printed text on a plain frame. */
export const textCard = style([
  face,
  {
    display: "flex",
    flexDirection: "column",
    gap: "6PX",
    padding: "12PX 12PX 10PX",
    border: "6PX solid #0b0d11",
    background: "linear-gradient(180deg, #efe6cf, #ddd2b6)",
    color: "#1b1710",
    textAlign: "left",
    overflow: "hidden",
    boxSizing: "border-box",
  },
]);
export const textName = style({
  font: `600 17PX/1.15 ${shellTokens.serif}`,
});
export const textType = style({
  font: `13PX/1.2 ${shellTokens.serif}`,
  paddingBottom: "5PX",
  borderBottom: "1PX solid #0003",
});
export const textRules = style({
  flex: 1,
  font: `12PX/1.25 ${shellTokens.serif}`,
  whiteSpace: "pre-line",
  overflow: "hidden",
});
export const textStats = style({
  alignSelf: "flex-end",
  font: `600 17PX/1 ${shellTokens.serif}`,
});

/** A library: the sleeve, with the cards beneath showing at its edge. */
export const pile = style({
  boxShadow:
    "2PX 2PX 0 #10131a, 4PX 4PX 0 #1b2030, 6PX 6PX 0 #10131a, 8PX 8PX 0 #1b2030, 0 16PX 26PX #000a",
});
export const badge = style({
  position: "absolute",
  right: "-8PX",
  bottom: "-8PX",
  minWidth: "44PX",
  padding: "6PX 10PX",
  borderRadius: "999PX",
  border: `2PX solid ${gold}`,
  background: "#0c1016f2",
  color: ivory,
  font: `600 22PX/1 ${shellTokens.serif}`,
  textAlign: "center",
  pointerEvents: "none",
});
export const countBadge = style([
  badge,
  { right: "auto", left: "-8PX", bottom: "-8PX" },
]);
/** Kept upright on a card that lies on its side. */
export const upright = style({ transform: "rotate(-90deg)" });

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
