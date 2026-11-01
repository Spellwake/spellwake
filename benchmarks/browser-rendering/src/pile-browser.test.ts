import { expect, test } from "bun:test";
import * as THREE from "three";
import { createPileBrowser, librarySamples, pileCard } from "./pile-browser";
import { createCardInspection } from "./card-inspection";
import { createFanFocus } from "./compact-fan";
import { foldedFan } from "./folded-fan";

test("library inspection preserves upper surfaces, including a face-up third card", () => {
  const entries = librarySamples(5);
  expect(entries[0]).toEqual({ key: null, faceUp: false });
  expect(entries[1]).toEqual({ key: null, faceUp: false });
  expect(entries[2]).toEqual({ key: "serra-angel", faceUp: true });
  expect(entries[3]).toEqual({ key: null, faceUp: false });
  const source = new THREE.Group(),
    front = new THREE.Mesh();
  front.userData.printedFront = true;
  source.add(front);
  const material = new THREE.MeshBasicMaterial();
  material.name = "Serra Angel";
  const materials = new Map([["serra-angel", material]]);
  const hidden = pileCard(
    source,
    { key: "serra-angel", faceUp: false },
    materials,
  );
  expect(hidden.userData.faceDown).toBe(true);
  expect(hidden.children).toHaveLength(0);
  const revealed = pileCard(
    source,
    { key: "serra-angel", faceUp: true },
    materials,
  );
  expect(revealed.userData.faceDown).toBe(false);
  expect(revealed.children).toHaveLength(1);
  expect(source.children[0]).toBe(front);
});

test("fold focus traverses the whole library and stops at its ends", () => {
  const scroll = createFanFocus();
  scroll.configure(200);
  scroll.begin();
  scroll.pan(0.4, 0.02);
  expect(scroll.position).toBeCloseTo(0.4);
  scroll.pan(65, 0.1);
  expect(scroll.position).toBeCloseTo(65.4);
  scroll.end(false);
  scroll.tick(0.1, false);
  expect(scroll.position).toBeGreaterThan(65.4);
  scroll.move(1000);
  expect(scroll.position).toBe(199);
  scroll.move(-1000);
  expect(scroll.position).toBe(0);
  scroll.begin();
  scroll.pan(2, 0.1);
  scroll.end(false);
  scroll.tick(0.1, true);
  expect(scroll.position).toBe(2);
});

test("pile overflow folds continuously at both ends without dropping entries", () => {
  for (const count of [0, 1, 4, 8, 52, 200]) {
    for (const focus of [0, count / 2, count - 1]) {
      const poses = foldedFan(count, focus);
      expect(poses).toHaveLength(count);
      expect(poses.every((pose) => Math.abs(pose.x) < 4.16)).toBe(true);
      if (count > 7) {
        expect(poses.filter((pose) => !pose.folded).length).toBeLessThanOrEqual(
          7,
        );
        expect(poses.some((pose) => pose.folded)).toBe(true);
      }
    }
  }
  const start = foldedFan(52, 0),
    end = foldedFan(52, 51);
  expect(start[0]!.folded).toBe(false);
  expect(start.at(-1)!.folded).toBe(true);
  expect(end[0]!.folded).toBe(true);
  expect(end.at(-1)!.folded).toBe(false);
  const before = foldedFan(52, 25),
    after = foldedFan(52, 25.001);
  for (const [i, pose] of before.entries())
    expect(Math.abs(after[i]!.x - pose.x)).toBeLessThan(0.002);
});

test("mixed-face pile sleeves do not intersect and selection alone comes forward", () => {
  for (const count of [3, 40, 200]) {
    const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
    const template = new THREE.Group();
    template.add(new THREE.Mesh(new THREE.BoxGeometry(2.5, 3.5, 0.05)));
    const material = new THREE.MeshBasicMaterial();
    const browser = createPileBrowser(
      camera,
      createCardInspection(camera),
      () => template,
      new Map([["serra-angel", material]]),
      () => {},
    );
    const pile = new THREE.Group();
    pile.userData.pileKind = "library";
    pile.userData.pileCards = librarySamples(count);
    browser.open(pile);
    browser.frame(1100, 1 / 60, false);
    const cards = camera.children[0]?.children;
    if (!cards || cards.length !== count) throw new Error("missing pile fan");
    camera.updateMatrixWorld(true);
    const visible = cards.filter((card) => card.visible);
    expect(visible).toHaveLength(count);
    for (const card of visible) {
      expect(card.scale.x).toBeCloseTo(visible[0]!.scale.x);
      expect(card.scale.y).toBeCloseTo(visible[0]!.scale.y);
    }
    const boxes = visible.map((card) => new THREE.Box3().setFromObject(card));
    for (let i = 0; i < boxes.length; i++)
      for (let j = i + 1; j < boxes.length; j++)
        expect(boxes[i]!.intersectsBox(boxes[j]!)).toBe(false);
    const maxDepth = Math.max(...cards.map((card) => card.position.z));
    expect(maxDepth).toBeLessThan(0.6);
    const before = cards.map((card) => card.position.clone());
    const ray = new THREE.Raycaster(
      new THREE.Vector3(),
      new THREE.Vector3(0, 0.45, -7).normalize(),
    );
    browser.hover(ray);
    browser.frame(1100, 1, true);
    expect(cards.some((card) => card.position.z > maxDepth)).toBe(true);
    expect(
      cards.filter((card, i) => card.position.distanceTo(before[i]!) > 0.001),
    ).toHaveLength(1);
    expect(cards[0]?.userData.faceDown).toBe(true);
    expect(cards[2]?.userData.faceDown).toBe(false);
  }
});

test("large pile navigation retains folded edges and every card identity", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const browser = createPileBrowser(
    camera,
    createCardInspection(camera),
    () => new THREE.Group(),
    new Map(),
    () => {},
  );
  const pile = new THREE.Group();
  pile.userData.pileKind = "library";
  pile.userData.pileCards = librarySamples(200);
  browser.open(pile);
  browser.frame(1100, 1, true);
  const cards = camera.children[0]?.children;
  if (!cards) throw new Error("missing pile fan");
  const original = [...cards];
  const visible = (): THREE.Object3D[] => cards.filter((card) => card.visible);
  expect(cards).toHaveLength(200);
  expect(visible()).toHaveLength(200);
  expect(cards.at(-1)!.position.x - cards[0]!.position.x).toBeGreaterThan(2);
  const initialPositions = cards.map((card) => card.position.clone());
  browser.move(1000);
  browser.frame(1100, 1, true);
  expect(visible()).toHaveLength(200);
  expect(cards[0]!.position.x).toBeLessThan(initialPositions[0]!.x);
  expect(cards.at(-1)!.position.x).toBeLessThan(initialPositions.at(-1)!.x);
  expect(cards).toEqual(original);
  expect(cards[0]!.userData.faceDown).toBe(true);
  expect(cards[2]!.userData.faceDown).toBe(true);
  browser.move(-1000);
  browser.frame(1100, 1, true);
  expect(visible()[0]?.userData.pileIndex).toBe(0);
});
