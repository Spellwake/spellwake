import { createVar, globalStyle } from "@vanilla-extract/css";
import { controlTokens as t } from "@spellwake/ui/tokens";

/**
 * 0 when surfaces are see-through, 1 when they must be solid. Everything
 * translucent in the application derives from this one switch, so a reader's
 * transparency or contrast preference changes the frame and the controls together.
 */
export const solid = createVar();
/** The blur and saturation behind a see-through surface, or none when solid. */
export const frost = createVar();

/** A surface colour that is `share` percent opaque over artwork and fully opaque when solid. */
const through = (colour: string, share: number): string =>
  `color-mix(in srgb, ${colour} calc(${share}% + ${100 - share}% * ${solid}), transparent)`;

/** Shared frame glass: the header, section rail, section strip and dock. */
export const glassSurface = {
  background: through("#0a1018", 46),
  backdropFilter: frost,
  WebkitBackdropFilter: frost,
  boxShadow: "inset 0 1px 0 #f6e9d314, inset 0 -1px 0 #00000059",
  "@media": {
    "(forced-colors: active)": { border: "1px solid CanvasText" },
  },
} as const;
export const glassEdge = "#f6e9d329";

const FROST = "blur(18px) saturate(165%)";
globalStyle("html:root", {
  vars: {
    [solid]: "0",
    [frost]: FROST,
    // Library control surfaces become glass through the same switch.
    [t.color.surface]: through("#15181e", 44),
    [t.color.field]: through("#0d1117", 48),
    [t.color.emphasis]: through("#2b2926", 52),
    [t.color.hover]: through("#24272b", 64),
    [t.color.headerFill]: through("#22272d", 50),
    [t.color.headerField]: through("#101821", 50),
    [t.color.primaryFill]:
      `linear-gradient(100deg,${through("#181a1d", 50)},${through("#20201d", 50)})`,
    // Menus, dialogs, tooltips and toasts keep the library's solid surfaces: overlays read badly as glass.
    [t.effect.backdrop]: frost,
  },
  "@supports": {
    // Without a blur, busy artwork would show through sharply behind text.
    "not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px)))":
      { vars: { [solid]: "0.8", [frost]: "none" } },
  },
  "@media": {
    "(prefers-reduced-transparency: reduce)": {
      vars: { [solid]: "1", [frost]: "none" },
    },
    "(prefers-contrast: more)": { vars: { [solid]: "1", [frost]: "none" } },
    "(forced-colors: active)": { vars: { [solid]: "1", [frost]: "none" } },
  },
});

/** Smallest touch target, in real pixels, whatever the display density. */
export const TOUCH_TARGET = "44PX";
export const touch = "(pointer: coarse)";
// Controls keep their designed size and never drop below a fingertip on touch screens.
globalStyle("html:root", {
  "@media": {
    [touch]: {
      vars: {
        [t.size.control]: `max(52px, ${TOUCH_TARGET})`,
        [t.size.compact]: `max(50px, ${TOUCH_TARGET})`,
        [t.size.compactAction]: `max(44px, ${TOUCH_TARGET})`,
      },
    },
  },
});
