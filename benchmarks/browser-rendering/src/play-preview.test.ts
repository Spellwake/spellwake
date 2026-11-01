import { expect, test } from "bun:test";
import * as THREE from "three";
import { createPlayPreview } from "./play-preview";

test("a land drop preserves its starting pose and approaches the owner's land row", () => {
  const scene = new THREE.Scene(),
    board = new THREE.Group(),
    card = new THREE.Group();
  board.position.set(10, 0, 20);
  board.rotation.y = Math.PI;
  card.position.set(1, 4, 30);
  card.userData.studyLand = true;
  scene.add(board, card);
  scene.updateMatrixWorld(true);
  const preview = createPlayPreview(
    scene,
    new THREE.PerspectiveCamera(35, 1, 0.1, 500),
  );
  preview.drop(card, board, 0);
  expect(card.position.toArray()).toEqual([1, 4, 30]);
  preview.tick(true, 1 / 60, false);
  expect(card.position.z).toBeGreaterThan(17.2);
  expect(card.position.z).toBeLessThan(30);
  for (let i = 0; i < 120; i++) preview.tick(true, 1 / 60, false);
  expect(card.position.x).toBeCloseTo(10);
  expect(card.position.z).toBeCloseTo(17.2);
  expect(card.userData.isHandCard).toBe(false);
  preview.clear();
  expect(preview.group.children).toHaveLength(0);
});

test("spells wait for explicit top-first resolution and preserve duplicate graveyard entries", () => {
  const scene = new THREE.Scene(),
    board = new THREE.Group(),
    grave = new THREE.Group(),
    first = new THREE.Group(),
    second = new THREE.Group();
  grave.userData.pileKind = "graveyard";
  grave.userData.pileEntries = ["serra-angel", "serra-angel"];
  board.add(grave);
  first.userData.studyFace = "first-spell";
  second.userData.studyFace = "second-spell";
  first.position.set(0, 3, 30);
  second.position.set(1, 3, 30);
  scene.add(board, first, second);
  scene.updateMatrixWorld(true);
  const preview = createPlayPreview(
    scene,
    new THREE.PerspectiveCamera(35, 1, 0.1, 500),
  );
  preview.drop(first, board, 0);
  preview.drop(second, board, 0);
  expect(preview.count).toBe(2);
  expect(preview.resolveTop()).toBe(false);
  for (let i = 0; i < 120; i++) preview.tick(true, 1 / 60, false);
  expect(preview.count).toBe(2);
  expect(preview.resolveTop()).toBe(true);
  expect(second.userData.pileKind).toBe("graveyard");
  expect(first.userData.pileKind).toBeUndefined();
  expect(grave.userData.pileEntries).toEqual([
    "second-spell",
    "serra-angel",
    "serra-angel",
  ]);
  expect(preview.resolveTop()).toBe(true);
  expect(preview.count).toBe(0);
  expect(grave.userData.pileEntries).toEqual([
    "first-spell",
    "second-spell",
    "serra-angel",
    "serra-angel",
  ]);
});

test("pending spells hover near the viewer across table zoom and settle back on resolution", () => {
  const scene = new THREE.Scene(),
    camera = new THREE.PerspectiveCamera(35, 1, 0.1, 500),
    board = new THREE.Group(),
    card = new THREE.Group();
  camera.position.set(0, 20, 25);
  camera.lookAt(0, 0, 0);
  scene.add(camera, board, card);
  scene.updateMatrixWorld(true);
  const preview = createPlayPreview(scene, camera);
  preview.drop(card, board, 0);
  preview.tick(true, 1 / 60, true);
  const near = camera.worldToLocal(card.getWorldPosition(new THREE.Vector3()));
  expect(near.z).toBeCloseTo(-6.5);
  expect(near.y).toBeCloseTo(0.2);
  expect(card.quaternion.angleTo(camera.quaternion)).toBeLessThan(0.2);
  camera.position.multiplyScalar(2);
  camera.updateMatrixWorld(true);
  preview.tick(true, 1 / 60, true);
  expect(
    camera.worldToLocal(card.getWorldPosition(new THREE.Vector3())).z,
  ).toBeCloseTo(-6.5);
  expect(preview.resolveTop()).toBe(true);
  preview.tick(true, 1 / 60, true);
  expect(card.position.y).toBeLessThan(0.5);
  expect(card.scale.x).toBeCloseTo(0.68);
});
