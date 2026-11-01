import { expect, test } from "bun:test";
import * as THREE from "three";
import { createTableCameraMotion, type TableCameraPose } from "./table-camera";
const home: TableCameraPose = {
  center: new THREE.Vector3(-17.5, 0, 5),
  yaw: 0,
  elevation: 1.1,
  distance: 28,
};
const away: TableCameraPose = {
  ...home,
  center: new THREE.Vector3(0, 0, -5),
  yaw: Math.PI,
};
test("seat travel pans and turns continuously, lifts out, and lands exactly", () => {
  const motion = createTableCameraMotion();
  motion.sample(home, 0, false);
  const start = motion.sample(away, 0, false);
  expect(start.center.toArray()).toEqual(home.center.toArray());
  expect(start.yaw).toBe(0);
  const middle = motion.sample(away, 0.425, false);
  expect(middle.yaw).toBeCloseTo(Math.PI / 2);
  expect(middle.center.x).toBeCloseTo(-8.75);
  expect(middle.distance).toBeGreaterThan(home.distance);
  expect(motion.moving).toBe(true);
  const end = motion.sample(away, 0.425, false);
  expect(end.center.toArray()).toEqual(away.center.toArray());
  expect(end.yaw).toBe(Math.PI);
  expect(end.distance).toBe(away.distance);
  expect(motion.moving).toBe(false);
});
test("retargeting starts from the visible pose; reduced motion snaps to destination", () => {
  const motion = createTableCameraMotion();
  motion.sample(home, 0, false);
  const interrupted = motion.sample(away, 0.3, false);
  const returning = motion.sample(home, 0, false);
  expect(returning.center.toArray()).toEqual(interrupted.center.toArray());
  expect(returning.yaw).toBe(interrupted.yaw);
  const reduced = motion.sample(home, 0, true);
  expect(reduced.center.toArray()).toEqual(home.center.toArray());
  expect(reduced.yaw).toBe(home.yaw);
  expect(motion.moving).toBe(false);
});
