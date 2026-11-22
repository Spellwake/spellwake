import { expect, test } from "bun:test";
import {
  CENTRED,
  MAX_ZOOM,
  coveringCrop,
  drawnSize,
  pannedCrop,
  zoomedCrop,
} from "../account/portrait-crop";

const LANDSCAPE = 626 / 457;

test("the artwork's shorter side fills the circle at zoom 1", () => {
  expect(drawnSize(1, LANDSCAPE)).toEqual({ width: LANDSCAPE, height: 1 });
  expect(drawnSize(1, 0.5)).toEqual({ width: 1, height: 2 });
  expect(drawnSize(2, 1)).toEqual({ width: 2, height: 2 });
});

test("at zoom 1 a landscape picture pans sideways only", () => {
  const moved = pannedCrop(CENTRED, 0.1, 0.4, LANDSCAPE);
  expect(moved.y).toBe(0.5);
  expect(moved.x).toBeCloseTo(0.5 - 0.1 / LANDSCAPE, 10);
});

test("a crop never shows past the artwork's edge", () => {
  for (const aspect of [LANDSCAPE, 1, 0.6])
    for (const scale of [1, 1.7, MAX_ZOOM])
      for (const [dx, dy] of [
        [9, 9],
        [-9, -9],
        [9, -9],
      ] as const) {
        const crop = pannedCrop({ ...CENTRED, scale }, dx, dy, aspect);
        const { width, height } = drawnSize(crop.scale, aspect);
        expect(crop.x * width).toBeGreaterThanOrEqual(0.5 - 1e-9);
        expect((1 - crop.x) * width).toBeGreaterThanOrEqual(0.5 - 1e-9);
        expect(crop.y * height).toBeGreaterThanOrEqual(0.5 - 1e-9);
        expect((1 - crop.y) * height).toBeGreaterThanOrEqual(0.5 - 1e-9);
      }
});

test("zooming out pulls a corner crop back inside the artwork", () => {
  const corner = pannedCrop({ ...CENTRED, scale: MAX_ZOOM }, -9, -9, LANDSCAPE);
  expect(corner.x).toBeGreaterThan(0.8);
  const out = zoomedCrop(corner, 1, LANDSCAPE);
  expect(out.y).toBe(0.5);
  expect(out.x).toBeCloseTo(1 - 0.5 / LANDSCAPE, 10);
});

test("zoom and position outside their ranges, or not numbers, are corrected", () => {
  expect(coveringCrop({ x: Number.NaN, y: 4, scale: 99 }, 1)).toEqual({
    x: 0.5,
    y: 1 - 0.5 / MAX_ZOOM,
    scale: MAX_ZOOM,
  });
  expect(coveringCrop({ x: 0.5, y: 0.5, scale: 0 }, 1).scale).toBe(1);
});
