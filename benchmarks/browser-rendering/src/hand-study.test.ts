import { expect, test } from "bun:test";
import * as THREE from "three";
import { createHandStudy, isHandLift } from "./hand-study";

test("an upward segment lifts after a long sideways browse without requiring a larger upward swipe", () => {
  expect(isHandLift(1000, 100, 1000, 700, 995)).toBe(false);
  expect(isHandLift(1000, 700, 995, 702, 970)).toBe(true);
  expect(isHandLift(1000, 700, 995, 702, 1015)).toBe(false);
  expect(isHandLift(1000, 700, 1000, 700, 990)).toBe(false);
});

test("hand and other-zone cards lift to the same readable height", () => {
  const material = new THREE.MeshBasicMaterial();
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(
    new THREE.Group(),
    boards,
    1,
    7,
    0,
    0,
    [7],
    [
      {
        availableCards: [
          { key: "krenko", faceUp: true, sourceZone: "command" },
          { key: "krenko", faceUp: true, sourceZone: "exile" },
        ],
      },
    ],
  );
  hands.frame(true, 1, 1, true);
  hands.hover(3);
  hands.frame(true, 1, 1, true);
  const height = hands.local.children[3]!.position.y;
  hands.hover(8);
  hands.frame(true, 1, 1, true);
  expect(hands.external.children[1]!.position.y).toBeCloseTo(height);
});

test("the raised card stays selected at its top while resting neighbor strips still retarget", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const template = new THREE.Group();
  template.add(
    new THREE.Mesh(
      new THREE.BoxGeometry(2.5, 3.49, 0.02),
      new THREE.MeshBasicMaterial(),
    ),
  );
  const hands = createHandStudy(camera, new THREE.MeshBasicMaterial());
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(template, boards, 1, 7, 0, 0);
  hands.frame(true, 1, 1, true);
  hands.hover(3);
  hands.frame(true, 1, 1, true);
  camera.updateMatrixWorld(true);
  const selected = hands.local.children[3]!;
  const point = selected.localToWorld(new THREE.Vector3(0, 1.65, 0.04));
  const origin = camera.getWorldPosition(new THREE.Vector3());
  expect(
    hands.pick(new THREE.Raycaster(origin, point.sub(origin).normalize())),
  ).toBe(3);
  const next = hands.local.children[4]!;
  const strip = hands.local.localToWorld(
    new THREE.Vector3(next.position.x - next.scale.x * 1.1, next.position.y, 0),
  );
  expect(
    hands.pick(new THREE.Raycaster(origin, strip.sub(origin).normalize())),
  ).toBe(4);
});

test("a huge hand retains every card in folded ends and slides to its last card", () => {
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    new THREE.MeshBasicMaterial(),
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  const template = new THREE.Group();
  template.add(new THREE.Mesh(new THREE.BoxGeometry(2.5, 3.49, 0.05)));
  hands.rebuild(template, boards, 1, 200, 0, 0);
  hands.frame(true, 1, 1, true);
  const original = [...hands.local.children];
  const visible = () => hands.local.children.filter((card) => card.visible);
  expect(visible()).toHaveLength(200);
  hands.beginPan();
  hands.pan(10000, 1100, 0.5);
  hands.endPan(true);
  hands.frame(true, 1, 1, true);
  expect(visible()).toContain(original.at(-1)!);
  expect(hands.currentCard).toBe(original.at(-1));
  expect(hands.local.children).toEqual(original);
  const gaps = visible()
    .slice(1)
    .map((card, index) => card.position.x - visible()[index]!.position.x);
  expect(Math.max(...gaps)).toBeGreaterThan(visible()[0]!.scale.x * 0.5);
  expect(Math.min(...gaps)).toBeLessThan(visible()[0]!.scale.x * 0.05);
  expect(visible()).toHaveLength(200);
  hands.local.updateWorldMatrix(true, true);
  const boxes = visible().map((card) => new THREE.Box3().setFromObject(card));
  for (let i = 0; i < boxes.length; i++)
    for (let j = i + 1; j < boxes.length; j++)
      expect(boxes[i]!.intersectsBox(boxes[j]!)).toBe(false);
});

test("dense-hand reordering uses actual indices and keeps other-zone cards pinned", () => {
  const material = new THREE.MeshBasicMaterial();
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 60, 0, 0);
  hands.frame(true, 1, 1, true);
  const commander = hands.external.children[0]!;
  const position = commander.position.clone();
  const original = [...hands.local.children];
  hands.beginPan(0);
  hands.pan(10000, 1100, 0.5);
  hands.endPan(true);
  hands.frame(true, 1, 1, true);
  expect(commander.visible).toBe(true);
  expect(commander.position.distanceTo(position)).toBeCloseTo(0);
  hands.beginDrag(55);
  hands.endDrag(58);
  hands.frame(true, 1, 1, true);
  expect(hands.local.children[58]).toBe(original[55]);
  expect(new Set(hands.local.children).size).toBe(60);
  expect(hands.local.children.filter((card) => card.visible)).toHaveLength(60);
});

test("a browsed huge hand reorders by visible drop position and can play the exact card", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(camera, new THREE.MeshBasicMaterial());
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 200, 0, 0);
  hands.frame(true, 1, 1, true);
  hands.beginPan(0);
  hands.pan(10000, 1100, 0.5);
  hands.endPan(true);
  hands.frame(true, 1, 1, true);
  const original = [...hands.local.children];
  const card = original[195]!;
  const destination = original[198]!;
  camera.updateMatrixWorld(true);
  hands.local.updateWorldMatrix(true, true);
  const origin = camera.getWorldPosition(new THREE.Vector3());
  const point = destination.getWorldPosition(new THREE.Vector3());
  const ray = new THREE.Raycaster(origin, point.sub(origin).normalize());
  expect(hands.beginDrag(195)).toBe(true);
  hands.dragTo(ray);
  expect(hands.slotAt(ray)).toBe(198);
  expect(hands.endDrag(hands.slotAt(ray))).toBe(card);
  hands.frame(true, 1, 1, true);
  expect(hands.local.children[198]).toBe(card);
  expect(new Set(hands.local.children)).toEqual(new Set(original));
  hands.beginDrag(198);
  expect(hands.canPlayDragged).toBe(true);
  const played = hands.endDrag(null);
  expect(played).toBe(card);
  played?.removeFromParent();
  hands.frame(true, 1, 1, true);
  expect(hands.local.children).toHaveLength(199);
  expect(hands.local.children).not.toContain(card);
  expect(hands.local.children.filter((entry) => entry.visible)).toHaveLength(
    199,
  );
});

test("a huge opponent hand browses and reorders without exposing hidden faces or granting play", () => {
  const material = new THREE.MeshBasicMaterial();
  const template = new THREE.Group();
  const face = new THREE.Mesh();
  face.userData.printedFront = true;
  template.add(face);
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
    new Map([["serra-angel", material]]),
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  hands.rebuild(
    template,
    boards,
    1,
    200,
    200,
    0,
    [200, 200],
    [
      {},
      {
        hand: Array.from({ length: 200 }, (_, index) =>
          index === 198 || index === 199
            ? { key: "serra-angel", faceUp: true, canPlay: index === 199 }
            : { key: null, faceUp: false },
        ),
      },
    ],
  );
  hands.frame(true, 1, 1, true);
  hands.beginPan(0);
  hands.pan(550, 1100, 0.5);
  hands.endPan(true);
  hands.frame(true, 1, 1, true);
  const ownVisible = hands.local.children.filter((card) => card.visible);
  hands.viewOpponent(2);
  hands.frame(true, 2, 1, true);
  const original = [...hands.active.children];
  expect(hands.canBrowse(0)).toBe(true);
  hands.beginPan(0);
  hands.pan(10000, 1100, 0.5);
  hands.endPan(true);
  hands.frame(true, 2, 1, true);
  expect(hands.currentIndex).toBe(199);
  expect(hands.active.children.filter((card) => card.visible)).toHaveLength(
    200,
  );
  expect(hands.currentCard).toBe(original[199]);
  hands.beginDrag(195);
  expect(hands.canPlayDragged).toBe(false);
  hands.endDrag(197);
  hands.frame(true, 2, 1, true);
  expect(hands.active.children[197]).toBe(original[195]);
  expect(new Set(hands.active.children)).toEqual(new Set(original));
  for (const card of original.slice(0, 198)) {
    expect(card.userData.faceDown).toBe(true);
    expect(card.userData.cardKey).toBeUndefined();
    expect(card.children.some((child) => child.userData.printedFront)).toBe(
      false,
    );
  }
  hands.beginDrag(198);
  expect(hands.canPlayDragged).toBe(false);
  hands.endDrag(null);
  hands.beginDrag(199);
  expect(hands.canPlayDragged).toBe(true);
  hands.endDrag(null);
  hands.viewOpponent(0);
  hands.frame(true, 1, 1, true);
  expect(hands.local.children.filter((card) => card.visible)).toEqual(
    ownVisible,
  );
  expect(hands.local.children).toHaveLength(200);
});

test("hands enter from below on opening and seat changes, with immediate reduced motion", () => {
  const material = new THREE.MeshBasicMaterial();
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group(), new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 7, 7, 0);
  hands.frame(true, 1, 0, false);
  const first = hands.local.children[0];
  if (!first) throw new Error("missing first card");
  const start = first.position.y;
  hands.frame(true, 1, 0.08, false);
  expect(first.position.y).toBeGreaterThan(start);
  hands.frame(true, 1, 1, false);
  const settled = first.position.y;
  expect(settled).toBeGreaterThan(start);
  hands.frame(true, 1, 0, false);
  expect(first.position.y).toBeCloseTo(settled);
  hands.frame(false, 1, 0, false);
  hands.frame(true, 1, 0, false);
  expect(first.position.y).toBeCloseTo(start);
  hands.frame(true, 1, 0, true);
  expect(first.position.y).toBeCloseTo(settled);
  hands.viewOpponent(2);
  hands.frame(true, 2, 0, false);
  const opponentStart = hands.viewedHand.children[0]?.position.y;
  hands.frame(true, 2, 1, false);
  expect(hands.viewedHand.children[0]!.position.y).toBeGreaterThan(
    opponentStart!,
  );
  hands.viewOpponent(3);
  hands.frame(true, 3, 0, true);
  expect(hands.viewedHand.children[0]!.position.y).toBeCloseTo(settled);
  expect(hands.local.children[0]).toBe(first);
});

test("opponent hand proxies cannot expose or inspect a printed face", () => {
  const template = new THREE.Group();
  const face = new THREE.Mesh();
  face.userData.printedFront = true;
  template.add(face);
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group(), new THREE.Group());
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(camera, new THREE.MeshBasicMaterial());
  hands.rebuild(template, boards, 2, 7, 5, 0);
  expect(hands.opponents.children.map((hand) => hand.userData.seat)).toEqual([
    1, 3,
  ]);
  expect(hands.local.children).toHaveLength(7);
  for (const hand of hands.opponents.children) {
    expect(hand.children).toHaveLength(5);
    hand.traverse((part) => {
      expect(part.userData.printedFront).not.toBe(true);
      expect(part.userData.isStudyCard).not.toBe(true);
    });
  }
  expect(template.children[0]).toBe(face);
  expect(face.visible).toBe(true);
  hands.frame(true, 1);
  expect(hands.opponents.children[0]?.visible).toBe(true);
  expect(hands.opponents.children[1]?.visible).toBe(false);
  hands.rebuild(template, boards, 1, 0, 0, 0);
  expect(hands.local.children).toHaveLength(0);
  expect(
    hands.opponents.children.every((hand) => hand.children.length === 0),
  ).toBe(true);
});

test("available cards stay beside the hand with source zones and independent play permissions", () => {
  const material = new THREE.MeshBasicMaterial();
  const template = new THREE.Group();
  const face = new THREE.Mesh();
  face.userData.printedFront = true;
  template.add(face);
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
    new Map([["serra-angel", material]]),
  );
  hands.rebuild(
    template,
    boards,
    1,
    7,
    0,
    0,
    [7, 0],
    [
      {
        availableCards: [
          {
            key: "serra-angel",
            faceUp: true,
            sourceZone: "exile",
            canPlay: true,
          },
          {
            key: "serra-angel",
            faceUp: true,
            sourceZone: "graveyard",
            canPlay: false,
          },
          { key: null, faceUp: false, sourceZone: "library", canPlay: true },
        ],
      },
      {},
    ],
  );
  hands.frame(true, 1, 1, true);
  expect(hands.local.children).toHaveLength(7);
  expect(
    hands.external.children.map((card) => card.userData.sourceZone),
  ).toEqual(["exile", "graveyard", "library"]);
  expect(hands.external.children[0]!.position.x).toBeGreaterThan(
    hands.local.children.at(-1)!.position.x,
  );
  expect(hands.external.children[0]!.position.y).toBeLessThan(
    hands.local.children.at(-1)!.position.y,
  );
  const card = hands.external.children[0]!;
  const baseline = card.position.y;
  hands.hover(7);
  hands.frame(true, 1, 1, true);
  expect(card.position.y).toBeGreaterThan(baseline);
  hands.beginDrag(7);
  expect(hands.canPlayDragged).toBe(true);
  hands.endDrag(0);
  expect(card.parent).toBe(hands.external);
  expect(hands.local.children).toHaveLength(7);
  hands.beginDrag(8);
  expect(hands.canPlayDragged).toBe(false);
  hands.endDrag(null);
  hands.beginDrag(9);
  expect(hands.canPlayDragged).toBe(false);
  hands.endDrag(null);
  expect(hands.external.children[2]?.userData.studyFace).toBeUndefined();
  hands.rebuild(
    template,
    boards,
    1,
    0,
    0,
    0,
    [0, 0],
    [
      {
        availableCards: [
          {
            key: "serra-angel",
            faceUp: true,
            sourceZone: "exile",
            canPlay: true,
          },
        ],
      },
      {},
    ],
  );
  hands.frame(true, 1, 1, true);
  const rayAt = (group: THREE.Group): THREE.Raycaster =>
    new THREE.Raycaster(
      new THREE.Vector3(),
      new THREE.Vector3(group.position.x, group.position.y, -9).normalize(),
    );
  expect(
    hands.pick(
      new THREE.Raycaster(
        new THREE.Vector3(),
        new THREE.Vector3(-3, hands.local.position.y, -9).normalize(),
      ),
    ),
  ).toBe(-1);
  expect(hands.pick(rayAt(hands.external))).toBe(0);
});

test("hover retargeting raises one card and settles the previous card gradually", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(camera, new THREE.MeshBasicMaterial());
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 3, 7, 0);
  hands.frame(true, 0, 1, true);
  const first = hands.local.children[0],
    second = hands.local.children[1];
  if (!first || !second) throw new Error("missing sample hand");
  const base = first.position.y;
  hands.hover(0);
  hands.frame(true, 0, 1 / 60, false);
  expect(first.position.y).toBeGreaterThan(base);
  expect(first.position.y).toBeLessThan(base + 0.3);
  for (let i = 0; i < 60; i++) hands.frame(true, 0, 1 / 60, false);
  const raised = first.position.y;
  hands.hover(1);
  hands.frame(true, 0, 1 / 60, false);
  expect(first.position.y).toBeLessThan(raised);
  expect(first.position.y).toBeGreaterThan(base);
  expect(second.position.y).toBeGreaterThan(0);
});

test("drag reorder preserves card identity and removing a played card reduces the hand", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(camera, new THREE.MeshBasicMaterial());
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 4, 0, 0);
  hands.frame(true, 0, 1, true);
  const original = [...hands.local.children];
  expect(hands.beginDrag(0)).toBe(true);
  expect(hands.endDrag(3)).toBe(original[0] ?? null);
  expect(hands.local.children).toEqual([
    ...original.slice(1),
    ...original.slice(0, 1),
  ]);
  hands.beginDrag(1);
  const played = hands.endDrag(null);
  played?.removeFromParent();
  expect(hands.local.children).toHaveLength(3);
  expect(new Set(hands.local.children).size).toBe(3);
});

test("hover slots follow the visible strips in an overlapping hand", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  camera.updateMatrixWorld();
  const hands = createHandStudy(camera, new THREE.MeshBasicMaterial());
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 7, 0, 0);
  hands.frame(true, 0, 1, true);
  const first = hands.local.children[0],
    second = hands.local.children[1];
  if (!first || !second) throw new Error("missing hand");
  const step = second.position.x - first.position.x;
  const stripX = first.position.x - first.scale.x * 1.25 + step * 1.3;
  const ray = new THREE.Raycaster(
    new THREE.Vector3(),
    new THREE.Vector3(stripX, hands.local.position.y, -9).normalize(),
  );
  expect(hands.pick(ray)).toBe(1);
});

test("commander is beside the hand and excluded from its count and reorder", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(
    camera,
    new THREE.MeshBasicMaterial(),
    new THREE.MeshBasicMaterial(),
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  const template = new THREE.Group();
  template.position.set(-8, 3, 14);
  template.rotation.x = -Math.PI / 2;
  hands.rebuild(template, boards, 1, 7, 7, 0);
  hands.frame(true, 0, 1, true);
  expect(hands.local.children).toHaveLength(7);
  expect(hands.external.children).toHaveLength(1);
  expect(
    hands.external.children[0]?.children.some(
      (part) => part.userData.commanderAura === true,
    ),
  ).toBe(true);
  expect(hands.external.children[0]?.rotation.x).toBe(0);
  expect(hands.external.children[0]?.userData.sourceZone).toBe("command");
  expect(hands.external.children[0]?.userData.isHandCard).toBe(false);
  expect(hands.external.children[0]!.position.x).toBeGreaterThan(
    hands.local.children.at(-1)!.position.x,
  );
  hands.beginDrag(0);
  hands.endDrag(6);
  expect(hands.external.children).toHaveLength(1);
  expect(hands.local.children).toHaveLength(7);
});

test("lifted cards stay under the pointer when commander spacing offsets the hand", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(
    camera,
    new THREE.MeshBasicMaterial(),
    new THREE.MeshBasicMaterial(),
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 20, 0, 0);
  hands.frame(true, 0, 1, true);
  hands.beginDrag(0);
  hands.dragTo(
    new THREE.Raycaster(
      new THREE.Vector3(),
      new THREE.Vector3(1, -1, -8.5).normalize(),
    ),
  );
  expect(
    hands.local.position.x + hands.local.children[0]!.position.x,
  ).toBeCloseTo(1);
  hands.endDrag(null);
  hands.browse(1);
  expect(hands.currentCard).toBe(hands.local.children[0]);
});

test("hover does not resize hand cards and the commander shares their scale", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(
    camera,
    new THREE.MeshBasicMaterial(),
    new THREE.MeshBasicMaterial(),
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 100, 0, 0);
  hands.frame(true, 0, 1, true);
  const size = hands.local.children[0]!.scale.x;
  const restingDepth = Math.max(
    ...hands.local.children.map((card) => card.position.z),
  );
  hands.hover(40);
  hands.frame(true, 0, 1, true);
  for (const card of hands.local.children.filter((card) => card.visible))
    expect(card.scale.x).toBeCloseTo(size);
  expect(hands.external.children[0]?.scale.x).toBeCloseTo(size);
  expect(hands.local.children[40]!.position.z).toBeGreaterThan(restingDepth);
});

test("viewing an opponent shows only authorized faces and preserves their command area", () => {
  const template = new THREE.Group();
  template.userData.studyFace = "private-template-face";
  const face = new THREE.Mesh();
  face.userData.printedFront = true;
  template.add(face);
  const material = new THREE.MeshBasicMaterial();
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
    new Map([
      ["serra-angel", material],
      ["krenko", material],
    ]),
  );
  hands.rebuild(
    template,
    boards,
    1,
    7,
    3,
    0,
    [7, 3],
    [
      {},
      {
        hand: [
          { key: null, faceUp: false },
          { key: "serra-angel", faceUp: true },
          { key: null, faceUp: false },
        ],
        command: [{ key: "krenko", faceUp: true }],
      },
    ],
  );
  hands.viewOpponent(2);
  hands.frame(true, 2, 1, true);
  expect(hands.viewedHand.children).toHaveLength(3);
  expect(hands.viewedExternal.children).toHaveLength(1);
  expect(hands.viewedHand.children[0]?.userData.studyFace).toBeUndefined();
  expect(hands.viewedHand.children[0]?.userData.faceDown).toBe(true);
  expect(hands.viewedHand.children[0]?.rotation.y).toBe(Math.PI);
  expect(
    hands.viewedHand.children[0]?.children.some(
      (part) => part.userData.printedFront,
    ),
  ).toBe(false);
  expect(hands.viewedHand.children[1]?.userData.studyFace).toBe("serra-angel");
  expect(hands.viewedHand.children[1]?.userData.faceDown).toBe(false);
  const [left, revealed, right] = hands.viewedHand.children;
  if (!left || !revealed || !right) throw new Error("missing mixed hand");
  expect(revealed.position.z).toBeCloseTo(
    (left.position.z + right.position.z) / 2,
  );
  const restingDepth = revealed.position.z;
  hands.hover(1);
  hands.frame(true, 2, 1, true);
  expect(revealed.position.z).toBeGreaterThan(restingDepth);
  hands.hover(-1);
  hands.frame(true, 2, 1, true);
  expect(revealed.position.z).toBeCloseTo(restingDepth);
  expect(hands.viewedExternal.children[0]?.userData.studyFace).toBe("krenko");
  expect(
    hands.viewedHand.children.every(
      (card) => card.userData.isHandCard === false,
    ),
  ).toBe(true);
  hands.viewOpponent(0);
  expect(hands.viewedHand.children).toHaveLength(0);
  expect(hands.local.children).toHaveLength(7);
});

test("opponent hand shares hover and reorder while playing requires explicit public permission", () => {
  const material = new THREE.MeshBasicMaterial();
  const template = new THREE.Group();
  const face = new THREE.Mesh();
  face.userData.printedFront = true;
  template.add(face);
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
    new Map([["serra-angel", material]]),
  );
  hands.rebuild(
    template,
    boards,
    1,
    7,
    3,
    0,
    [7, 3],
    [
      {},
      {
        hand: [
          { key: null, faceUp: false },
          { key: "serra-angel", faceUp: true },
          { key: "serra-angel", faceUp: true, canPlay: true },
        ],
      },
    ],
  );
  hands.viewOpponent(2);
  hands.frame(true, 2, 1, true);
  const hidden = hands.active.children[0];
  if (!hidden) throw new Error("missing public proxy");
  const base = hidden.position.y;
  hands.hover(0);
  hands.frame(true, 2, 1 / 60, false);
  expect(hidden.position.y).toBeGreaterThan(base);
  expect(hidden.position.y).toBeLessThan(base + 0.3);
  hands.beginDrag(0);
  expect(hands.canPlayDragged).toBe(false);
  hands.endDrag(1);
  expect(hands.active.children[1]).toBe(hidden);
  expect(hands.local.children).toHaveLength(7);
  hands.beginDrag(0);
  expect(hands.canPlayDragged).toBe(false);
  hands.endDrag(null);
  hands.beginDrag(2);
  expect(hands.canPlayDragged).toBe(true);
  hands.endDrag(null);
  hands.viewOpponent(0);
  hands.frame(true, 1, 1, true);
  expect(hands.active).toBe(hands.local);
});

test("folded-hand selection reaches every entry and preserves source-group anchors", () => {
  const material = new THREE.MeshBasicMaterial();
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    material,
    material,
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 200, 0, 0);
  hands.frame(true, 1, 1, true);
  const original = [...hands.local.children];
  const commander = hands.external.children[0]!;
  const anchor = commander.position.clone();
  for (let index = 0; index < 200; index++) {
    hands.browse(1);
    hands.frame(true, 1, 1, true);
    expect(hands.currentIndex).toBe(index);
    expect(hands.currentCard).toBe(original[index]);
    expect(original[index]!.visible).toBe(true);
    expect(hands.local.children.every((card) => card.visible)).toBe(true);
    expect(commander.position.distanceTo(anchor)).toBeCloseTo(0);
  }
  expect(hands.local.children).toEqual(original);
});

test("a grabbed card owns selection until release and settles the previous hover", () => {
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(36, 1, 1, 500),
    new THREE.MeshBasicMaterial(),
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 7, 0, 0);
  hands.frame(true, 1, 1, true);
  const resting = hands.local.children.map((card) => card.position.y);
  hands.hover(3);
  hands.frame(true, 1, 1, true);
  hands.beginDrag(4);
  hands.frame(true, 1, 1, true);
  expect(hands.local.children[3]!.position.y).toBeCloseTo(resting[3]!);
  expect(hands.currentCard).toBe(hands.local.children[4]);
  expect(hands.hover(5)).toBe(false);
  expect(hands.currentCard).toBe(hands.local.children[4]);
  hands.endDrag(null);
  expect(hands.hover(5)).toBe(true);
});

test("grabbing the raised face takes that card rather than a resting neighbor behind it", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const hands = createHandStudy(camera, new THREE.MeshBasicMaterial());
  const template = new THREE.Group();
  template.add(new THREE.Mesh(new THREE.BoxGeometry(2.5, 3.49, 0.02)));
  const boards = new THREE.Group();
  boards.add(new THREE.Group());
  hands.rebuild(template, boards, 1, 7, 0, 0);
  hands.frame(true, 1, 1, true);
  hands.hover(3);
  hands.frame(true, 1, 1, true);
  camera.updateMatrixWorld(true);
  const card = hands.local.children[3]!;
  const point = card.localToWorld(new THREE.Vector3(0.7, -1, 0.04));
  const origin = camera.getWorldPosition(new THREE.Vector3());
  expect(
    hands.hit(new THREE.Raycaster(origin, point.sub(origin).normalize())),
  ).toBe(3);
});

test("integrated own hands use supplied faces, opaque identities and permissions", () => {
  const camera = new THREE.PerspectiveCamera(36, 1, 1, 500);
  const original = new THREE.MeshBasicMaterial();
  const supplied = new THREE.MeshBasicMaterial();
  const template = new THREE.Group();
  const face = new THREE.Mesh(new THREE.PlaneGeometry(2.5, 3.5), original);
  face.userData.printedFront = true;
  template.add(face);
  const hands = createHandStudy(
    camera,
    original,
    undefined,
    new Map([["known", supplied]]),
    { fixture: false },
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  hands.rebuild(
    template,
    boards,
    1,
    2,
    0,
    0,
    [2, 0],
    [
      {
        hand: [
          {
            key: "known",
            faceUp: true,
            sourceId: "host-object",
            canPlay: false,
          },
          {
            key: "known",
            faceUp: true,
            sourceId: "host-playable",
            canPlay: true,
          },
        ],
      },
    ],
  );
  expect(hands.local.children[0]?.userData.sourceId).toBe("host-object");
  expect(hands.local.children[0]?.children[0]).toBeInstanceOf(THREE.Mesh);
  expect((hands.local.children[0]?.children[0] as THREE.Mesh).material).toBe(
    supplied,
  );
  hands.beginDrag(0);
  expect(hands.canPlayDragged).toBe(false);
  hands.endDrag(null);
  hands.beginDrag(1);
  expect(hands.canPlayDragged).toBe(true);
});

test("integration never synthesizes command offers or play permission from a command display", () => {
  const material = new THREE.MeshBasicMaterial();
  const hands = createHandStudy(
    new THREE.PerspectiveCamera(),
    material,
    material,
    new Map([["krenko", material]]),
    { fixture: false },
  );
  const boards = new THREE.Group();
  boards.add(new THREE.Group(), new THREE.Group());
  hands.rebuild(new THREE.Group(), boards, 1, 0, 0, 0, [0, 0], [{}]);
  expect(hands.external.children.length).toBe(0);
  hands.rebuild(
    new THREE.Group(),
    boards,
    1,
    0,
    0,
    0,
    [0, 0],
    [{ command: [{ key: "krenko", faceUp: true }] }],
  );
  expect(hands.external.children.length).toBe(1);
  hands.beginDrag(0);
  expect(hands.canPlayDragged).toBe(false);
});
