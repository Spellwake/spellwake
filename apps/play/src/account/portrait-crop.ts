/**
 * A circular crop of one piece of catalogue artwork.
 *
 * `x` and `y` place the circle's centre as a fraction of the artwork's width
 * and height. `scale` is the zoom: at 1 the circle's diameter equals the
 * artwork's shorter side. These are the values the account stores, so a crop
 * drawn at any size shows the same picture.
 */
export type PortraitCrop = { x: number; y: number; scale: number };

export const MIN_ZOOM = 1;
export const MAX_ZOOM = 3;
export const CENTRED: PortraitCrop = { x: 0.5, y: 0.5, scale: 1 };

const clamp = (value: number, low: number, high: number): number =>
  Math.min(high, Math.max(low, value));

/**
 * The artwork's drawn size at a zoom, in units of the circle's diameter.
 * `aspect` is the artwork's width over its height.
 */
export function drawnSize(
  scale: number,
  aspect: number,
): { width: number; height: number } {
  return aspect >= 1
    ? { width: scale * aspect, height: scale }
    : { width: scale, height: scale / aspect };
}

/**
 * The nearest crop that leaves no empty area inside the square around the
 * circle. A crop is always shown and saved through this, so a picture can
 * never include pixels the artwork does not have.
 */
export function coveringCrop(crop: PortraitCrop, aspect: number): PortraitCrop {
  const scale = clamp(
    Number.isFinite(crop.scale) ? crop.scale : MIN_ZOOM,
    MIN_ZOOM,
    MAX_ZOOM,
  );
  const { width, height } = drawnSize(scale, aspect);
  // Half the circle, as a fraction of the drawn artwork, must fit on each side.
  const marginX = 0.5 / width;
  const marginY = 0.5 / height;
  return {
    x: clamp(Number.isFinite(crop.x) ? crop.x : 0.5, marginX, 1 - marginX),
    y: clamp(Number.isFinite(crop.y) ? crop.y : 0.5, marginY, 1 - marginY),
    scale,
  };
}

/**
 * Moves the artwork under the circle. `dx` and `dy` are how far the pointer
 * travelled, in units of the circle's diameter: dragging right brings the
 * artwork's left side into view.
 */
export function pannedCrop(
  crop: PortraitCrop,
  dx: number,
  dy: number,
  aspect: number,
): PortraitCrop {
  const { width, height } = drawnSize(crop.scale, aspect);
  return coveringCrop(
    { ...crop, x: crop.x - dx / width, y: crop.y - dy / height },
    aspect,
  );
}

/** Changes the zoom about the circle's centre. */
export function zoomedCrop(
  crop: PortraitCrop,
  scale: number,
  aspect: number,
): PortraitCrop {
  return coveringCrop({ ...crop, scale }, aspect);
}
