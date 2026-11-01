import { expect, test } from "bun:test";
import * as THREE from "three";
import { createCardInspection } from "./card-inspection";

test("inspection travels from source without a view cut and returns the original", () => {
  const scene = new THREE.Scene(),
    camera = new THREE.PerspectiveCamera(36, 1, 1, 500),
    source = new THREE.Group();
  scene.add(camera, source);
  source.position.set(2, -1, -15);
  scene.updateMatrixWorld(true);
  const inspection = createCardInspection(camera);
  inspection.open(source);
  expect(source.visible).toBe(false);
  const copy = camera.children[0];
  expect(copy?.position.z).toBe(-15);
  expect(inspection.tick(1 / 60, false)).toBe(true);
  expect(copy?.position.z).toBeGreaterThan(-15);
  expect(copy?.position.z).toBeLessThan(-5);
  for (let i = 0; i < 90; i++) inspection.tick(1 / 60, false);
  expect(copy?.position.z).toBe(-5);
  expect(inspection.active).toBe(true);
  inspection.close();
  inspection.tick(1 / 60, false);
  expect(copy?.position.z).toBeLessThan(-5);
  expect(source.visible).toBe(false);
  for (let i = 0; i < 90; i++) inspection.tick(1 / 60, false);
  expect(source.visible).toBe(true);
  expect(camera.children).toHaveLength(0);
  expect(inspection.active).toBe(false);
  inspection.open(source);
  inspection.tick(1 / 60, true);
  expect(camera.children[0]?.position.z).toBe(-5);
  inspection.close();
  inspection.tick(1 / 60, true);
  expect(source.visible).toBe(true);
});
