export { DeckBox } from "./DeckBox";
export type { DeckBoxProps } from "./DeckBox";
export type { BoxAppearance, ManaColor } from "./box-appearance";
export { MANA_ORDER, DEFAULT_APPEARANCE } from "./box-appearance";
export type { BoxMotion } from "./box-motion";

/** Keep Three.js lazy for consumers that only render the collection trigger. */
export async function mountBoxMotion(
  ...parameters: Parameters<typeof import("./box-motion").mountBoxMotion>
): ReturnType<typeof import("./box-motion").mountBoxMotion> {
  const renderer = await import("./box-motion");
  return renderer.mountBoxMotion(...parameters);
}
export {
  readBoxView,
  DEFAULT_BOX_VIEW,
  collectionViewUrl,
  type BoxView,
} from "./box-view";
