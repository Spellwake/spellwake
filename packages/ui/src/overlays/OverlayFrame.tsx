import { useId, useLayoutEffect, useRef, type RefObject } from "react";
import * as s from "./overlays.css";

export const pointer = { base: 16, depth: 8, gap: 4, corner: 10 } as const;

/** Trace the pointed edge as part of the outline so no stroke crosses the mouth. */
function outline(
  box: DOMRect,
  marker: DOMRect | undefined,
  side: string | undefined,
) {
  const halfStroke = 0.5,
    right = box.width - halfStroke,
    bottom = box.height - halfStroke;
  const r = Math.min(pointer.corner, right / 2, bottom / 2),
    half = pointer.base / 2;
  const bounded = (center: number, length: number) =>
    Math.max(r + half, Math.min(length - r - half, center));
  const x = marker
    ? bounded(marker.x + marker.width / 2 - box.x, box.width)
    : 0;
  const y = marker
    ? bounded(marker.y + marker.height / 2 - box.y, box.height)
    : 0;
  const depth = pointer.depth;
  return [
    `M ${halfStroke + r} ${halfStroke}`,
    side === "bottom" && marker
      ? `H ${x - half} l ${half} ${-depth} ${half} ${depth}`
      : "",
    `H ${right - r} q ${r} 0 ${r} ${r}`,
    side === "left" && marker
      ? `V ${y - half} l ${depth} ${half} ${-depth} ${half}`
      : "",
    `V ${bottom - r} q 0 ${r} ${-r} ${r}`,
    side === "top" && marker
      ? `H ${x + half} l ${-half} ${depth} ${-half} ${-depth}`
      : "",
    `H ${halfStroke + r} q ${-r} 0 ${-r} ${-r}`,
    side === "right" && marker
      ? `V ${y + half} l ${-depth} ${-half} ${depth} ${-half}`
      : "",
    `V ${halfStroke + r} q 0 ${-r} ${r} ${-r} Z`,
  ].join(" ");
}

/** Radix or Floating UI places the invisible marker. A single filled path owns both the face and its edge. */
export function OverlayFrame({
  marker,
}: {
  marker: RefObject<SVGSVGElement | null>;
}) {
  const path = useRef<SVGPathElement>(null),
    gradient = useId();
  useLayoutEffect(() => {
    const edge = path.current,
      svg = edge?.ownerSVGElement,
      panel = svg?.parentElement;
    if (!edge || !svg || !panel) return;
    const anchor = marker.current;
    const draw = () => {
      const bounds = svg.getBoundingClientRect();
      if (!bounds.width || !bounds.height) return;
      const point =
        anchor && getComputedStyle(anchor).visibility !== "hidden"
          ? anchor.getBoundingClientRect()
          : undefined;
      edge.setAttribute("d", outline(bounds, point, panel.dataset.side));
    };
    const size = new ResizeObserver(draw);
    size.observe(svg);
    const placement = new MutationObserver(draw);
    placement.observe(panel, {
      attributes: true,
      attributeFilter: ["data-side"],
    });
    if (anchor) {
      placement.observe(anchor, {
        attributes: true,
        attributeFilter: ["style"],
      });
      if (anchor.parentElement && anchor.parentElement !== panel)
        placement.observe(anchor.parentElement, {
          attributes: true,
          attributeFilter: ["style"],
        });
    }
    draw();
    return () => {
      size.disconnect();
      placement.disconnect();
    };
  }, [marker]);
  return (
    <svg aria-hidden="true" focusable="false" className={s.frame}>
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2=".85" y2="1">
          <stop stopColor="#232a2c" />
          <stop offset=".44" stopColor="#141c20" />
          <stop offset="1" stopColor="#181c1e" />
        </linearGradient>
      </defs>
      <path
        ref={path}
        fill={`url(#${gradient})`}
        stroke="#a99d88"
        strokeWidth="1"
        strokeLinejoin="round"
      />
    </svg>
  );
}
