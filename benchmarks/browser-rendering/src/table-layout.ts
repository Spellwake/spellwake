/** Stable physical seating and owner-relative boards with viewer-relative card faces. */
import * as THREE from "three";
import { createClassicBoard } from "./classic-board";

/** Keep a card fixed to the table, allowing only a half-turn for readability. */
export function readableCardTurn(viewYaw: number): number {
  return Math.cos(viewYaw) < -1e-8 ? Math.PI : 0;
}

export type LayoutSeat = { x: number; z: number; yaw: number; name: string };
/** Seat controls belong beyond the owner's hand edge, in the owner's facing direction. */
export function tableSeatAnchor(seat: Pick<LayoutSeat, "x" | "z" | "yaw">): {
  x: number;
  z: number;
} {
  return {
    x: seat.x + Math.sin(seat.yaw) * 8,
    z: seat.z + Math.cos(seat.yaw) * 8,
  };
}
export type TableMode = "ffa" | "one-two" | "two-one" | "two-two";
type PhysicalSeat = readonly [number, number, number];
// Immutable clockwise seat order. View changes rotate the table, never permute seats.
const arrangements: Record<number, readonly PhysicalSeat[]> = {
  2: [
    [0, 5, 0],
    [0, -5, Math.PI],
  ],
  3: [
    [0, 5, 0],
    [-8.75, -5, Math.PI],
    [8.75, -5, Math.PI],
  ],
  4: [
    [-8.75, 5, 0],
    [-8.75, -5, Math.PI],
    [8.75, -5, Math.PI],
    [8.75, 5, 0],
  ],
  5: [
    [-8.75, 5, 0],
    [-17.5, -5, Math.PI],
    [0, -5, Math.PI],
    [17.5, -5, Math.PI],
    [8.75, 5, 0],
  ],
  6: [
    [-17.5, 5, 0],
    [-17.5, -5, Math.PI],
    [0, -5, Math.PI],
    [17.5, -5, Math.PI],
    [17.5, 5, 0],
    [0, 5, 0],
  ],
};
function physicalSeats(
  count: number,
  mode: TableMode,
): readonly PhysicalSeat[] {
  if (count === 3 && mode === "two-one")
    return [
      [-8.75, 5, 0],
      [0, -5, Math.PI],
      [8.75, 5, 0],
    ];
  return arrangements[count] ?? arrangements[2] ?? [];
}
function teamFor(index: number, mode: TableMode): string {
  if (mode === "one-two") return index === 0 ? "A" : "B";
  if (mode === "two-one") return index === 1 ? "B" : "A";
  if (mode === "two-two") return index === 0 || index === 3 ? "A" : "B";
  return "";
}

/** Clone shared approved meshes into stable seat regions; no game state is inferred. */
export function createTableLayout(
  surfaceY: number,
  landMaterial: THREE.Material,
  boardMaterials: ReadonlyMap<string, THREE.Material>,
) {
  const group = new THREE.Group();
  const matGeometry = new THREE.BoxGeometry(16.4, 0.08, 10);
  const matMaterial = new THREE.MeshStandardMaterial({
    color: 0x18312e,
    roughness: 0.96,
  });
  const localMaterial = new THREE.MeshStandardMaterial({
    color: 0x263b37,
    roughness: 0.96,
  });
  let seats: LayoutSeat[] = [];
  const bounds = new THREE.Box3();
  const focusBounds = new THREE.Box3();
  const boards: THREE.Group[] = [];
  function rebuild(
    template: THREE.Group,
    count: number,
    viewer: number,
    mode: TableMode,
  ): void {
    group.traverse((object) => {
      if (object instanceof THREE.InstancedMesh) object.dispose();
    });
    group.clear();
    boards.length = 0;
    const positions = physicalSeats(count, mode);
    const viewingAngle = positions[viewer - 1]?.[2] ?? 0;
    group.rotation.y = -viewingAngle;
    const rotationAxis = new THREE.Vector3(0, 1, 0);
    seats = positions.map(([x, z], i) => {
      const anchor = new THREE.Vector3(x, 0, z).applyAxisAngle(
        rotationAxis,
        -viewingAngle,
      );
      const team = teamFor(i, mode);
      return {
        x: anchor.x,
        z: anchor.z,
        yaw: (positions[i]?.[2] ?? 0) - viewingAngle,
        name: `${i === viewer - 1 ? "You · P" + (i + 1) : "Player " + (i + 1)}${team ? " · Team " + team : ""}`,
      };
    });
    for (const [seatIndex, position] of positions.entries()) {
      const [x, z, ownerAngle] = position;
      const board = new THREE.Group();
      board.userData.seat = seatIndex + 1;
      board.position.set(x, 0, z);
      board.rotation.y = ownerAngle;
      const mat = new THREE.Mesh(
        matGeometry,
        seatIndex === viewer - 1 ? localMaterial : matMaterial,
      );
      mat.position.y = surfaceY - 0.04;
      mat.receiveShadow = true;
      mat.visible = false; // Invisible region bounds, not a default playmat.
      board.add(mat);
      const samples = createClassicBoard(
        template,
        landMaterial,
        boardMaterials,
        surfaceY,
      );
      // Keep positions/zones owner-relative; compensate only each individual card's turn.
      samples.traverse((sample) => {
        if (
          sample.userData.isStudyCard !== true ||
          sample instanceof THREE.InstancedMesh
        )
          return;
        sample.rotation.z +=
          sample.userData.faceDown === true
            ? ownerAngle - viewingAngle
            : viewingAngle - ownerAngle;
      });
      board.add(samples);
      group.add(board);
      boards.push(board);
    }
    bounds.setFromObject(group);
  }
  return {
    /** View a seat without changing its identity, neighbors, or private hand. */
    perspective(focus: number, facingYaw?: number): number {
      const board = boards[focus - 1];
      const yaw = board ? board.rotation.y + group.rotation.y : 0;
      group.traverse((card) => {
        if (
          card.userData.isStudyCard !== true ||
          card instanceof THREE.InstancedMesh
        )
          return;
        if (typeof card.userData.basePerspectiveRotation !== "number")
          card.userData.basePerspectiveRotation = card.rotation.z;
        card.rotation.z =
          Number(card.userData.basePerspectiveRotation) +
          readableCardTurn(facingYaw ?? yaw);
      });
      return yaw;
    },
    group,
    rebuild,
    get seats(): readonly LayoutSeat[] {
      return seats;
    },
    contentBounds(focus: number): THREE.Box3 {
      const board = boards[focus - 1];
      const contents = board?.children[1];
      return contents ? focusBounds.setFromObject(contents) : bounds;
    },
    bounds(focus: number): THREE.Box3 {
      const board = boards[focus - 1];
      return board ? focusBounds.setFromObject(board) : bounds;
    },
  };
}
