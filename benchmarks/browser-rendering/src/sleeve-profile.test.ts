import { expect, test } from "bun:test";
import * as THREE from "three";
import {
  createPressedSleeveSeam,
  type SleeveFilmProfile,
} from "./sleeve-profile";

const contour = [
  new THREE.Vector2(-1, -2),
  new THREE.Vector2(1, -2),
  new THREE.Vector2(1, 2),
  new THREE.Vector2(-1, 2),
  new THREE.Vector2(-1, -2),
];
const profile: SleeveFilmProfile = {
  panelScaleX: 0.95,
  panelScaleY: 0.975,
  panelOffsetY: 0.05,
  backZ: -0.0136,
  frontZ: 0.012,
  sealThickness: 0.002,
  openingY: 1.999,
};

test("pressed seal fits both offset panels and compresses only the outer margin", () => {
  const geometry = createPressedSleeveSeam(contour, profile);
  const positions = geometry.getAttribute("position");
  const points = Array.from({ length: positions.count }, (_, i) =>
    new THREE.Vector3().fromBufferAttribute(positions, i),
  );
  for (const panelZ of [profile.frontZ, profile.backZ]) {
    for (const point of contour.slice(0, -1))
      expect(
        points.some(
          (p) =>
            Math.abs(p.x - point.x * profile.panelScaleX) < 1e-6 &&
            Math.abs(
              p.y - (point.y * profile.panelScaleY + profile.panelOffsetY),
            ) < 1e-6 &&
            Math.abs(p.z - panelZ) < 1e-6,
        ),
      ).toBe(true);
  }
  const weld = points.filter(
    (p) => Math.abs(Math.abs(p.x) - 1) < 1e-6 || Math.abs(p.y + 2) < 1e-6,
  );
  const depth =
    Math.max(...weld.map((p) => p.z)) - Math.min(...weld.map((p) => p.z));
  expect(depth).toBeCloseTo(profile.sealThickness, 7);
  expect(depth).toBeLessThan((profile.frontZ - profile.backZ) / 10);
  geometry.dispose();
});

test("the sleeve opening has no closing triangles and shading normals remain finite", () => {
  const geometry = createPressedSleeveSeam(contour, profile);
  const positions = geometry.getAttribute("position");
  const index = geometry.getIndex();
  expect(index).not.toBeNull();
  if (!index) throw new Error("Expected indexed sleeve geometry");
  for (let i = 0; i < index.count; i += 3) {
    const ys = [index.getX(i), index.getX(i + 1), index.getX(i + 2)].map(
      (vertex) => positions.getY(vertex),
    );
    expect(ys.every((y) => y > profile.openingY)).toBe(false);
  }
  expect(
    Array.from(geometry.getAttribute("normal").array).every(Number.isFinite),
  ).toBe(true);
  geometry.dispose();
});
