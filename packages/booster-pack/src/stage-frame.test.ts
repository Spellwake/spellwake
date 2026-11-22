import { describe, expect, test } from "bun:test";
import { CAMERA, framePack, placeRect, worldPerPixel } from "./stage-frame";

const canvas = { width: 1440, height: 900 };

describe("placeRect", () => {
  test("puts the canvas centre at the origin", () => {
    const place = placeRect(
      { x: 620, y: 400, width: 200, height: 100 },
      canvas,
    );
    expect(place.x).toBeCloseTo(0, 9);
    expect(place.y).toBeCloseTo(0, 9);
  });
  test("projects back onto the same page pixels through the camera", () => {
    const rect = { x: 52, y: 120, width: 184, height: 257 };
    const place = placeRect(rect, canvas);
    const half = Math.tan(((CAMERA.fov / 2) * Math.PI) / 180) * CAMERA.distance;
    const toPixelX = (x: number): number =>
      ((x / (half * (canvas.width / canvas.height)) + 1) / 2) * canvas.width;
    const toPixelY = (y: number): number =>
      ((1 - y / half) / 2) * canvas.height;
    expect(toPixelX(place.x - place.width / 2)).toBeCloseTo(rect.x, 6);
    expect(toPixelY(place.y + place.height / 2)).toBeCloseTo(rect.y, 6);
    expect(place.width / worldPerPixel(canvas.height)).toBeCloseTo(
      rect.width,
      6,
    );
  });
});

describe("framePack", () => {
  const pack = { width: 1, height: 420 / 238 };
  test("fits the height of a wide area", () => {
    const area = { x: 0, y: 100, width: 1440, height: 700 };
    const frame = framePack(area, canvas, pack, 0.74, 0.62);
    expect((frame.scale * pack.height) / worldPerPixel(900)).toBeCloseTo(
      700 * 0.74,
      6,
    );
  });
  test("fits the width of a narrow area", () => {
    const narrow = { width: 375, height: 812 };
    const area = { x: 0, y: 80, width: 375, height: 650 };
    const frame = framePack(area, narrow, pack, 0.74, 0.62);
    expect(frame.scale / worldPerPixel(812)).toBeCloseTo(375 * 0.62, 6);
  });
});
