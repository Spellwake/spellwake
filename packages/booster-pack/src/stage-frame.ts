/**
 * The stage's fixed camera looks straight at the z = 0 plane. Anything placed
 * on that plane maps to page pixels by one scale, which is what lets a dealt
 * card land exactly on its slot in the page.
 */

/** A page rectangle in CSS pixels, relative to the canvas. */
export type Rect = { x: number; y: number; width: number; height: number };
/** A point and size on the z = 0 plane, in world units. */
export type Placement = { x: number; y: number; width: number; height: number };

export const CAMERA = { fov: 30, distance: 6 } as const;

/** World units per CSS pixel on the z = 0 plane, for a canvas this tall. */
export function worldPerPixel(canvasHeight: number): number {
  const visible =
    2 * CAMERA.distance * Math.tan(((CAMERA.fov / 2) * Math.PI) / 180);
  return visible / Math.max(1, canvasHeight);
}

/** Where a page rectangle sits on the z = 0 plane. */
export function placeRect(
  rect: Rect,
  canvas: { width: number; height: number },
): Placement {
  const k = worldPerPixel(canvas.height);
  return {
    x: (rect.x + rect.width / 2 - canvas.width / 2) * k,
    y: (canvas.height / 2 - (rect.y + rect.height / 2)) * k,
    width: rect.width * k,
    height: rect.height * k,
  };
}

/**
 * The pack's scale and centre for an area of the page: as large as fits
 * within `fill` of the area's height and `fillWidth` of its width.
 */
export function framePack(
  area: Rect,
  canvas: { width: number; height: number },
  pack: { width: number; height: number },
  fill = 0.74,
  fillWidth = 0.62,
): { x: number; y: number; scale: number } {
  const place = placeRect(area, canvas);
  const scale = Math.min(
    (place.height * fill) / pack.height,
    (place.width * fillWidth) / pack.width,
  );
  return { x: place.x, y: place.y, scale: Math.max(scale, 0.01) };
}
