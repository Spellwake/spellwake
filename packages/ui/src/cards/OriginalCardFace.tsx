import type { ReactNode } from "react";
import * as s from "./cards.css";

export interface CardImage {
  src: string;
  name: string;
  width: number;
  height: number;
}
/** Coordinates are in pixels of the supplied, uncropped image. */
export interface PrintedRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}
export function regionStyle(region: PrintedRegion, image: CardImage) {
  return {
    left: (100 * region.x) / image.width + "%",
    top: (100 * region.y) / image.height + "%",
    width: (100 * region.width) / image.width + "%",
    height: (100 * region.height) / image.height + "%",
  };
}
export interface OriginalCardFaceProps {
  image: CardImage;
  highlights?: readonly {
    id: string;
    region: PrintedRegion;
    tone: "available" | "selected";
  }[];
  children?: ReactNode;
}
/** Keeps the full original scan and its overlays in the same coordinate space. */
export function OriginalCardFace({
  image,
  highlights,
  children,
}: OriginalCardFaceProps) {
  return (
    <div
      className={s.face}
      style={{ aspectRatio: image.width + "/" + image.height }}
    >
      <img
        className={s.scan}
        src={image.src}
        alt={image.name}
        width={image.width}
        height={image.height}
        draggable={false}
      />
      {highlights?.map((h) => (
        <span
          aria-hidden
          key={h.id}
          className={s.mark}
          data-tone={h.tone}
          style={regionStyle(h.region, image)}
        />
      ))}
      {children}
    </div>
  );
}
