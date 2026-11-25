import { useId, useMemo, useRef, type CSSProperties, type PointerEvent } from "react";
import {
  renderCardFrameSvg,
  type CardFaceFrameInput,
  type StackTriggerFrameInput,
} from "./card-frame-svg";

export interface CustomCardFrameProps {
  input: CardFaceFrameInput | StackTriggerFrameInput;
  className?: string;
  style?: CSSProperties;
  /** Enable direct drag-to-pan framing inside the artwork window. */
  artInteractive?: boolean;
  onArtPanDelta?: (deltaXFraction: number, deltaYFraction: number) => void;
  onArtZoomWheel?: (deltaZoom: number) => void;
  ariaLabel?: string;
}

/**
 * Live interactive React view over the unified 1062x1482 procedural SVG card renderer.
 * Supports direct pointer drag-to-pan and wheel zoom in the Custom Card Studio.
 */
export function CustomCardFrame({
  input,
  className,
  style,
  artInteractive = false,
  onArtPanDelta,
  onArtZoomWheel,
  ariaLabel,
}: CustomCardFrameProps) {
  const reactId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const dragRef = useRef<{ x: number; y: number } | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const svgMarkup = useMemo(
    () =>
      renderCardFrameSvg({
        ...input,
        idPrefix: input.idPrefix ?? `sw-${reactId}`,
      }),
    [input, reactId],
  );

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (!artInteractive || !onArtPanDelta) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = { x: event.clientX, y: event.clientY };
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!artInteractive || !onArtPanDelta || !dragRef.current || !containerRef.current) {
      return;
    }
    const rect = containerRef.current.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;
    const dx = (event.clientX - dragRef.current.x) / rect.width;
    const dy = (event.clientY - dragRef.current.y) / rect.height;
    dragRef.current = { x: event.clientX, y: event.clientY };
    onArtPanDelta(dx, dy);
  };

  const handlePointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (dragRef.current) {
      dragRef.current = null;
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    }
  };

  const label =
    ariaLabel ??
    (input.kind === "card_face"
      ? `${input.face.name || "Untitled Card"} custom card preview`
      : `${input.name} stack ability`);

  return (
    <div
      ref={containerRef}
      role="img"
      aria-label={label}
      className={className}
      style={{
        aspectRatio: "1062 / 1482",
        width: "100%",
        userSelect: "none",
        cursor: artInteractive ? "grab" : undefined,
        touchAction: artInteractive ? "none" : undefined,
        ...style,
      }}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      onWheel={(event) => {
        if (!artInteractive || !onArtZoomWheel) return;
        event.preventDefault();
        onArtZoomWheel(event.deltaY < 0 ? 0.1 : -0.1);
      }}
      // Self-contained SVG built strictly from XML-escaped strings and bundled Scryfall vectors.
      dangerouslySetInnerHTML={{ __html: svgMarkup }}
    />
  );
}
