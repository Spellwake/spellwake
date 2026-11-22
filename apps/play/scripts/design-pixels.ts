import type { Declaration, Plugin } from "postcss";

/** Root font size the design pixel scale assumes. */
const ROOT_PIXELS = 16;
const PIXELS = /"[^"]*"|'[^']*'|url\([^)]*\)|(-?\d*\.?\d+)px\b/g;

/**
 * Shared controls and screens are authored in the pixel scale of the approved
 * reference rasters. This rewrites those stylesheet lengths to rem, so the
 * application sets display density with the root font size alone. Scripted
 * measurements, popup positions and canvas sizes stay in real pixels, which is
 * why page zoom and transforms are not used for this.
 *
 * Hairlines of one pixel or less stay in pixels so borders remain crisp, and
 * media queries are left alone because they are not declarations. A length
 * written with an upper-case unit, such as 44PX, is a real device-independent
 * pixel and is never rewritten: use it for limits that must not scale, like
 * the minimum touch target.
 */
export function designPixels(): Plugin {
  return {
    postcssPlugin: "spellwake-design-pixels",
    Declaration(declaration: Declaration) {
      if (!declaration.value.includes("px")) return;
      declaration.value = declaration.value.replace(
        PIXELS,
        (match, amount: string | undefined) => {
          if (amount === undefined) return match;
          const pixels = Number(amount);
          if (Math.abs(pixels) <= 1) return match;
          return `${Number((pixels / ROOT_PIXELS).toFixed(5))}rem`;
        },
      );
    },
  };
}
