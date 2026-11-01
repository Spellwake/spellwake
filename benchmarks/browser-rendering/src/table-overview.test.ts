import { expect, test } from "bun:test";
import { overviewDots } from "./table-overview";
test("icon preserves physical rows, seat order and the fixed viewer", () => {
  const seats = [
    { x: -17.5, z: 5 },
    { x: -17.5, z: -5 },
    { x: 0, z: -5 },
    { x: 17.5, z: -5 },
    { x: 17.5, z: 5 },
    { x: 0, z: 5 },
  ].map((seat, index) => ({ ...seat, name: String(index + 1) }));
  const dots = overviewDots(seats, 6);
  expect(dots.map((dot) => dot.seat)).toEqual([1, 2, 3, 4, 5, 6]);
  expect(dots.filter((dot) => dot.y === 13).length).toBe(3);
  expect(dots.filter((dot) => dot.y === 35).length).toBe(3);
  expect(dots.find((dot) => dot.viewer)?.seat).toBe(6);
  expect(dots.every((dot) => Math.hypot(dot.x - 24, dot.y - 24) < 20)).toBe(
    true,
  );
});
test("duel icon centers aligned seats and three-seat teams retain their rows", () => {
  expect(
    overviewDots(
      [
        { x: 0, z: 5, name: "You" },
        { x: 0, z: -5, name: "Other" },
      ],
      1,
    ).map((dot) => dot.x),
  ).toEqual([24, 24]);
  const dots = overviewDots(
    [
      { x: -8.75, z: 5, name: "You" },
      { x: 0, z: -5, name: "Opponent" },
      { x: 8.75, z: 5, name: "Ally" },
    ],
    3,
  );
  expect(dots[0]?.y).toBe(dots[2]?.y);
  expect(dots[1]?.y).toBeLessThan(dots[0]?.y ?? 0);
  expect(dots[2]?.viewer).toBe(true);
});

import * as THREE from "three";
import {
  createTableLayout,
  tableSeatAnchor,
  type TableMode,
} from "./table-layout";
test("every supported count and team layout produces one bounded dot for each fixed seat", () => {
  const material = new THREE.MeshBasicMaterial();
  for (const [count, mode] of [
    [2, "ffa"],
    [3, "ffa"],
    [3, "one-two"],
    [3, "two-one"],
    [4, "ffa"],
    [4, "two-two"],
    [5, "ffa"],
    [6, "ffa"],
  ] as const satisfies readonly (readonly [number, TableMode])[]) {
    const layout = createTableLayout(0, material, new Map());
    for (let viewer = 1; viewer <= count; viewer++) {
      layout.rebuild(new THREE.Group(), count, viewer, mode);
      const dots = overviewDots(layout.seats, viewer);
      for (const seat of layout.seats) {
        const anchor = tableSeatAnchor(seat);
        expect(Math.hypot(anchor.x - seat.x, anchor.z - seat.z)).toBeCloseTo(8);
        expect(Math.abs(anchor.z)).toBeGreaterThan(Math.abs(seat.z));
      }
      expect(dots.length).toBe(count);
      expect(dots.filter((dot) => dot.viewer).map((dot) => dot.seat)).toEqual([
        viewer,
      ]);
      expect(new Set(dots.map((dot) => `${dot.x},${dot.y}`)).size).toBe(count);
      expect(
        dots.every(
          (dot) =>
            Number.isFinite(dot.x) &&
            Number.isFinite(dot.y) &&
            Math.hypot(dot.x - 24, dot.y - 24) < 20,
        ),
      ).toBe(true);
    }
  }
});
