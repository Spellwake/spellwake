import { useLayoutEffect, useState, type RefObject } from "react";

/** Where a named card or portrait sits, measured from an overlay's corner. */
export interface AnchorRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}
export interface Anchors {
  rects: ReadonlyMap<string, AnchorRect>;
  /** The overlay's own size. */
  width: number;
  height: number;
}
const NONE: Anchors = { rects: new Map(), width: 0, height: 0 };

const same = (a: Anchors, b: Anchors): boolean => {
  if (a.width !== b.width || a.height !== b.height) return false;
  if (a.rects.size !== b.rects.size) return false;
  for (const [name, rect] of a.rects) {
    const other = b.rects.get(name);
    if (
      !other ||
      Math.abs(other.left - rect.left) > 0.2 ||
      Math.abs(other.top - rect.top) > 0.2 ||
      Math.abs(other.width - rect.width) > 0.2 ||
      Math.abs(other.height - rect.height) > 0.2
    )
      return false;
  }
  return true;
};

/**
 * Follows the elements named by `data-anchor` (`card:<handle>` or
 * `seat:<seatId>`), and the pile's own items (`stack:<handle>`), so a line or
 * a control can be attached to the actual card, portrait or spell wherever
 * the camera puts it. Anything not on screen is absent.
 */
export function useAnchors(
  root: RefObject<Element | null>,
  names: readonly string[],
  /** Changes whenever the cards may have moved. */
  follow: unknown,
): Anchors {
  const [anchors, setAnchors] = useState<Anchors>(NONE);
  const wanted = names.join("|");
  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    let frame = 0;
    let settled = 0;
    let frames = 0;
    let until = 0;
    let latest = NONE;
    const measure = () => {
      const origin = element.getBoundingClientRect();
      const rects = new Map<string, AnchorRect>();
      for (const name of wanted ? wanted.split("|") : []) {
        // `frame:<part>` names a part of the screen's own frame instead,
        // and `stack:<handle>` a spell or ability in the pile.
        const node = document.querySelector<HTMLElement>(
          name.startsWith("frame:")
            ? `[data-frame="${CSS.escape(name.slice(6))}"]`
            : name.startsWith("stack:")
              ? `[data-match-stack] [data-item="${CSS.escape(name.slice(6))}"]`
              : `[data-anchor="${CSS.escape(name)}"]`,
        );
        const rect = node?.getBoundingClientRect();
        if (!rect || rect.width === 0) continue;
        rects.set(name, {
          left: rect.left - origin.left,
          top: rect.top - origin.top,
          right: rect.right - origin.left,
          bottom: rect.bottom - origin.top,
          width: rect.width,
          height: rect.height,
        });
      }
      const next = { rects, width: origin.width, height: origin.height };
      settled = same(latest, next) ? settled + 1 : 0;
      latest = next;
      setAnchors((current) => (same(current, next) ? current : next));
      // Keep following while the cards travel to a new view. A window left in
      // the background draws no frames, so the clock alone cannot end this:
      // the cards must also have stood still for a few frames in a row.
      frames += 1;
      if ((performance.now() < until || settled < 8) && frames < 900)
        frame = requestAnimationFrame(measure);
    };
    const restart = () => {
      cancelAnimationFrame(frame);
      settled = 0;
      frames = 0;
      until = performance.now() + 900;
      measure();
    };
    restart();
    window.addEventListener("resize", restart);
    document.addEventListener("visibilitychange", restart);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", restart);
      document.removeEventListener("visibilitychange", restart);
    };
  }, [root, wanted, follow]);
  return anchors;
}
