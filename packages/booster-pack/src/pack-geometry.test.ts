import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import { PACK, PackSheet, tearLine, thickness } from "./pack-geometry";

describe("thickness", () => {
  test("is flat in the crimps and at the folds, full across the face", () => {
    expect(thickness(0, PACK.height / 2 - PACK.crimp / 2)).toBeCloseTo(
      PACK.seal,
      9,
    );
    expect(thickness(0, -PACK.height / 2 + PACK.crimp / 2)).toBeCloseTo(
      PACK.seal,
      9,
    );
    expect(thickness(PACK.width / 2, 0)).toBeCloseTo(PACK.seal, 9);
    expect(thickness(0, 0)).toBeCloseTo(PACK.seal + PACK.puff, 9);
  });
});

describe("tear line", () => {
  test("stays between the crimp and the face", () => {
    for (let x = -0.5; x <= 0.5; x += 0.01) {
      expect(tearLine(x)).toBeLessThan(PACK.height / 2 - PACK.crimp);
      expect(tearLine(x)).toBeGreaterThan(PACK.height / 2 - PACK.crimp - 0.1);
    }
  });
});

describe("PackSheet", () => {
  test("body and strip meet exactly on the tear", () => {
    const body = new PackSheet(1, "body");
    const strip = new PackSheet(1, "strip");
    const top = new Set<string>();
    const positions = body.geometry.getAttribute("position");
    for (let index = 0; index < positions.count; index += 1) {
      const x = positions.getX(index);
      const y = positions.getY(index);
      if (Math.abs(y - tearLine(x)) < 1e-6)
        top.add(`${x.toFixed(5)}:${y.toFixed(5)}`);
    }
    const stripPositions = strip.geometry.getAttribute("position");
    let shared = 0;
    for (let index = 0; index < stripPositions.count; index += 1) {
      const key = `${stripPositions.getX(index).toFixed(5)}:${stripPositions.getY(index).toFixed(5)}`;
      if (top.has(key)) shared += 1;
    }
    expect(top.size).toBeGreaterThan(80);
    expect(shared).toBe(top.size);
    body.dispose();
    strip.dispose();
  });
  test("normals face outward on both sheets", () => {
    for (const side of [1, -1] as const) {
      const sheet = new PackSheet(side, "body");
      const normals = sheet.geometry.getAttribute("normal");
      const positions = sheet.geometry.getAttribute("position");
      const n = new THREE.Vector3();
      for (let index = 0; index < normals.count; index += 1) {
        if (
          Math.abs(positions.getX(index)) > 0.3 ||
          Math.abs(positions.getY(index)) > 0.5
        )
          continue;
        n.fromBufferAttribute(normals, index);
        expect(Math.sign(n.z)).toBe(side);
        expect(n.length()).toBeCloseTo(1, 5);
      }
      sheet.dispose();
    }
  });
});
