import { useEffect, useRef, type ReactElement, type RefObject } from "react";
import {
  mountPackStage,
  type PackStage,
  type Rect,
} from "@spellwake/booster-pack";
import { motionLanguage as m } from "../../lib/motion";
import { packPrint } from "./pack-palette";
import { CARD_BACK, useSetSymbol } from "./pack-assets";
import * as s from "./packs.css";

/** A viewport rectangle, relative to the stage's canvas. */
function toCanvas(rect: Rect, canvas: HTMLElement): Rect {
  const origin = canvas.getBoundingClientRect();
  return { ...rect, x: rect.x - origin.left, y: rect.y - origin.top };
}

/**
 * The 3D wrapper behind an opening: it lifts off the shelf, waits while the
 * opening resolves, then tears open and deals the cards onto their slots.
 * Purely visual; the page's own cards take over once they land. If 3D is
 * unavailable, `onUnavailable` hands the opening back to the page.
 */
export function OpeningStage({
  setName,
  setCode,
  product,
  from,
  areaRef,
  slots,
  finishRef,
  onLanded,
  onUnavailable,
}: {
  setName: string;
  setCode: string | null;
  product: string;
  /** The wrapper the pack was opened from, in viewport pixels. */
  from: Rect | undefined;
  /** The part of the stage the waiting pack is framed in. */
  areaRef: RefObject<HTMLElement | null>;
  /** The card slots to deal onto, in viewport pixels, once the cards are known. */
  slots: readonly Rect[] | null;
  /** Receives a function that lands every card at once. */
  finishRef: RefObject<(() => void) | null>;
  onLanded: () => void;
  onUnavailable: () => void;
}): ReactElement {
  const host = useRef<HTMLDivElement>(null);
  const stage = useRef<PackStage | null>(null);
  const ready = useRef<(() => void) | null>(null);
  const latest = useRef({ onLanded, onUnavailable, slots });
  latest.current = { onLanded, onUnavailable, slots };

  // One stage per opening: the props it starts from are read once, after
  // the set's symbol is known, so the wrapper is printed only once.
  const symbol = useSetSymbol(setCode, setName);
  const start = useRef({ setName, product, from });
  useEffect(() => {
    const element = host.current;
    const area = areaRef.current;
    if (!element || !symbol.isSettled) return;
    const controller = new AbortController();
    const { setName: name, product: title, from: origin } = start.current;
    let observer: ResizeObserver | undefined;
    const frame = (mounted: PackStage): void => {
      if (!area) return;
      const box = area.getBoundingClientRect();
      // A removed area reports an empty box; keep the last real one.
      if (box.width === 0 || box.height === 0) return;
      mounted.setArea(
        toCanvas(
          { x: box.left, y: box.top, width: box.width, height: box.height },
          element,
        ),
      );
    };
    mountPackStage(element, {
      print: packPrint(name, title, symbol.url),
      curves: {
        arrive: m.ease.arrive,
        depart: m.ease.depart,
        travel: m.ease.travel,
      },
      beats: {
        scene: m.duration.scene,
        hero: m.duration.hero,
        stagger: m.stagger,
      },
      cardBackUrl: CARD_BACK,
      from: origin && toCanvas(origin, element),
      signal: controller.signal,
    })
      .then((mounted) => {
        if (controller.signal.aborted) {
          mounted.dispose();
          return;
        }
        stage.current = mounted;
        finishRef.current = mounted.finish;
        frame(mounted);
        if (area) {
          observer = new ResizeObserver(() => frame(mounted));
          observer.observe(area);
        }
        ready.current?.();
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        console.error("Unable to stage the pack in 3D.", error);
        latest.current.onUnavailable();
      });
    return () => {
      controller.abort();
      observer?.disconnect();
      stage.current?.dispose();
      stage.current = null;
      finishRef.current = null;
    };
  }, [areaRef, finishRef, symbol.isSettled, symbol.url]);

  // Deal once both the stage and the slots exist, whichever comes last.
  useEffect(() => {
    if (!slots) return;
    let cancelled = false;
    const deal = (): void => {
      const mounted = stage.current;
      const element = host.current;
      if (!mounted || !element || cancelled) return;
      ready.current = null;
      const local = slots.map((slot) => toCanvas(slot, element));
      void mounted.open(local).then(() => {
        if (!cancelled) latest.current.onLanded();
      });
    };
    if (stage.current) deal();
    else ready.current = deal;
    return () => {
      cancelled = true;
      ready.current = null;
    };
  }, [slots]);

  return <div ref={host} className={s.stageCanvas} aria-hidden="true" />;
}
