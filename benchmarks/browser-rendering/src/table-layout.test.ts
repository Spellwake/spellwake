import { expect, test } from "bun:test";
import * as THREE from "three";
import { createTableLayout, readableCardTurn } from "./table-layout";

test("opponent perspectives preserve seat order, board positions and card poses on return", () => {
  const material = new THREE.MeshBasicMaterial();
  const template = new THREE.Group();
  for (let i = 0; i < 6; i++) {
    const card = new THREE.Mesh(
      new THREE.BoxGeometry(2.5, 3.49, 0.02),
      material,
    );
    card.userData.isStudyCard = true;
    template.add(card);
  }
  const table = createTableLayout(0, material, new Map());
  table.rebuild(template, 6, 1, "ffa");
  table.perspective(1);
  const seats = structuredClone(table.seats);
  const positions = table.group.children.map((board) =>
    board.position.toArray(),
  );
  const poses: number[][] = [];
  table.group.traverse((card) => {
    if (card.userData.isStudyCard === true)
      poses.push(card.rotation.toArray().slice(0, 3).map(Number));
  });
  for (const seat of [2, 6, 3, 5, 4, 2, 1]) table.perspective(seat);
  expect(table.seats).toEqual(seats);
  expect(table.group.children.map((board) => board.position.toArray())).toEqual(
    positions,
  );
  const returned: number[][] = [];
  table.group.traverse((card) => {
    if (card.userData.isStudyCard === true)
      returned.push(card.rotation.toArray().slice(0, 3).map(Number));
  });
  expect(returned).toEqual(poses);
  expect(table.perspective(2)).toBeCloseTo(Math.PI);
  expect(table.perspective(6)).toBeCloseTo(0);
});

test("card readability permits only a half-turn instead of following the moving camera", () => {
  for (const yaw of [0, 0.2, 0.8, Math.PI / 2, -0.8, -Math.PI / 2, 2 * Math.PI])
    expect(readableCardTurn(yaw)).toBe(0);
  for (const yaw of [Math.PI / 2 + 0.01, 2, Math.PI, -2, -Math.PI])
    expect(readableCardTurn(yaw)).toBe(Math.PI);
});
