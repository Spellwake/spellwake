import * as THREE from "three";
import { smoothstep } from "./pack-timeline";

/**
 * The pack's shape, in world units where the wrapper is one unit wide. Its
 * proportions follow the 2D wrapper (238 × 420); the crimp is 5.4% of the
 * height, as drawn there.
 */
export const PACK = (() => {
  const width = 1;
  const height = 420 / 238;
  const crimp = height * 0.054;
  /** How far below the top crimp the wrapper tears. */
  const tearDepth = 0.05;
  return {
    width,
    height,
    crimp,
    /** Where the pillow rises from the flat crimp to full thickness. */
    shoulder: 0.19,
    /** Half the thickness at the fullest point. */
    puff: 0.052,
    /** Front and back stay this far apart in the crimps, so they never fight for depth. */
    seal: 0.0018,
    tearY: height / 2 - crimp - tearDepth,
    /** How far the opened mouth gapes at its centre, per sheet. */
    mouth: 0.085,
  } as const;
})();

/** Cards inside, as a share of the pack's width (a 63 mm card in a 75 mm wrapper). */
export const CARD = (() => {
  const width = PACK.width * 0.84;
  return { width, height: (width * 88) / 63, thickness: 0.0034 } as const;
})();

/** A ragged but repeatable tear: a slow waver plus fine fibres. */
export function tearOffset(x: number): number {
  const slow =
    0.007 * Math.sin(x * 5.3 + 0.7) + 0.004 * Math.sin(x * 11.9 + 2.1);
  const fibre =
    0.0028 * Math.sin(x * 61 + 1.3) +
    0.0018 * Math.sin(x * 137 + 0.4) +
    0.0012 * Math.sin(x * 263 + 5.1);
  return slow + fibre;
}
export const tearLine = (x: number): number => PACK.tearY + tearOffset(x);

/**
 * Half-thickness of the sealed pillow at (x, y): flat in the crimps, full
 * across the face, and closing to a rounded fold at the sides.
 */
export function thickness(x: number, y: number): number {
  const u = Math.min(1, Math.abs((2 * x) / PACK.width));
  const across = Math.sqrt(Math.max(0, 1 - u ** 6));
  const fromBottom = y + PACK.height / 2;
  const fromTop = PACK.height / 2 - y;
  const start = PACK.crimp;
  const end = PACK.crimp + PACK.shoulder;
  const along =
    smoothstep(start, end, fromBottom) * smoothstep(start, end, fromTop);
  return PACK.seal + PACK.puff * across * along;
}

export type Side = 1 | -1;
export type Region = "body" | "strip";
/** Where a point of the flat print sits once deformed. */
export type Deform = (
  x: number,
  y: number,
  side: Side,
  out: THREE.Vector3,
) => THREE.Vector3;

/** The rest shape: each sheet bowed out by the pillow's thickness. */
export const restShape: Deform = (x, y, side, out) =>
  out.set(x, y, side * thickness(x, y));

const COLUMNS = 84;
const ROWS: Record<Region, number> = { body: 104, strip: 10 };

/** Columns crowd towards the folds, where the surface turns fastest. */
function columnX(column: number): number {
  const t = (column / COLUMNS) * 2 - 1;
  return (Math.sin((t * Math.PI) / 2) * PACK.width) / 2;
}

/**
 * One sheet (front or back) of one region (the body below the tear, or the
 * strip above it). Both regions share the tear line exactly, so the sealed
 * pack reads as one surface.
 */
export class PackSheet {
  readonly geometry = new THREE.BufferGeometry();
  /** Flat print coordinates of every vertex, which deformations map from. */
  private readonly flat: Float32Array;
  private readonly position: THREE.BufferAttribute;
  private readonly normal: THREE.BufferAttribute;
  private readonly a = new THREE.Vector3();
  private readonly b = new THREE.Vector3();
  private readonly c = new THREE.Vector3();
  private readonly d = new THREE.Vector3();
  private readonly n = new THREE.Vector3();

  constructor(
    readonly side: Side,
    readonly region: Region,
  ) {
    const rows = ROWS[region];
    const count = (COLUMNS + 1) * (rows + 1);
    this.flat = new Float32Array(count * 2);
    const uv = new Float32Array(count * 2);
    for (let column = 0; column <= COLUMNS; column += 1) {
      const x = columnX(column);
      const tear = tearLine(x);
      const bottom = region === "body" ? -PACK.height / 2 : tear;
      const top = region === "body" ? tear : PACK.height / 2;
      for (let row = 0; row <= rows; row += 1) {
        const index = column * (rows + 1) + row;
        const y = bottom + ((top - bottom) * row) / rows;
        this.flat[index * 2] = x;
        this.flat[index * 2 + 1] = y;
        // The back is printed as it is seen from behind.
        uv[index * 2] =
          side === 1 ? x / PACK.width + 0.5 : 0.5 - x / PACK.width;
        uv[index * 2 + 1] = y / PACK.height + 0.5;
      }
    }
    const indices: number[] = [];
    for (let column = 0; column < COLUMNS; column += 1) {
      for (let row = 0; row < rows; row += 1) {
        const a = column * (rows + 1) + row;
        const b = (column + 1) * (rows + 1) + row;
        const c = a + 1;
        const d = b + 1;
        if (side === 1) indices.push(a, b, c, c, b, d);
        else indices.push(a, c, b, c, d, b);
      }
    }
    this.position = new THREE.BufferAttribute(new Float32Array(count * 3), 3);
    this.normal = new THREE.BufferAttribute(new Float32Array(count * 3), 3);
    this.geometry.setAttribute("position", this.position);
    this.geometry.setAttribute("normal", this.normal);
    this.geometry.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
    // The flat print position, for shader effects tied to the tear.
    this.geometry.setAttribute(
      "printPosition",
      new THREE.BufferAttribute(this.flat, 2),
    );
    this.geometry.setIndex(indices);
    this.update(restShape);
  }

  /**
   * Re-pose vertices through `deform`. Normals come from the deformed surface
   * itself, so lighting follows peels and gapes without seams. Vertices below
   * `fromY` are left as they are, for deformations that never reach them.
   */
  update(deform: Deform, fromY = -Infinity): void {
    const h = 0.0025;
    const half = PACK.width / 2;
    const positions = this.position.array as Float32Array;
    const normals = this.normal.array as Float32Array;
    const count = this.flat.length / 2;
    for (let index = 0; index < count; index += 1) {
      const x = this.flat[index * 2] ?? 0;
      const y = this.flat[index * 2 + 1] ?? 0;
      if (y < fromY) continue;
      deform(x, y, this.side, this.a);
      positions[index * 3] = this.a.x;
      positions[index * 3 + 1] = this.a.y;
      positions[index * 3 + 2] = this.a.z;
      const left = Math.max(-half, x - h);
      const right = Math.min(half, x + h);
      deform(left, y, this.side, this.b);
      deform(right, y, this.side, this.c);
      this.b.sub(this.c).negate();
      deform(x, y - h, this.side, this.c);
      deform(x, y + h, this.side, this.d);
      this.d.sub(this.c);
      this.n.crossVectors(this.b, this.d).multiplyScalar(this.side);
      if (this.n.lengthSq() < 1e-14) this.n.set(0, 0, this.side);
      this.n.normalize();
      normals[index * 3] = this.n.x;
      normals[index * 3 + 1] = this.n.y;
      normals[index * 3 + 2] = this.n.z;
    }
    this.position.needsUpdate = true;
    this.normal.needsUpdate = true;
    this.geometry.computeBoundingSphere();
  }

  dispose(): void {
    this.geometry.dispose();
  }
}

/**
 * A card's outline, with corners rounded like the printed card (the DOM slot
 * rounds 5% by 3.6%), in a unit-width, card-height frame centred on zero.
 */
export function cardGeometry(): THREE.ShapeGeometry {
  const w = CARD.width;
  const h = CARD.height;
  const rx = w * 0.05;
  const ry = h * 0.036;
  const shape = new THREE.Shape();
  shape.moveTo(-w / 2 + rx, -h / 2);
  shape.lineTo(w / 2 - rx, -h / 2);
  shape.quadraticCurveTo(w / 2, -h / 2, w / 2, -h / 2 + ry);
  shape.lineTo(w / 2, h / 2 - ry);
  shape.quadraticCurveTo(w / 2, h / 2, w / 2 - rx, h / 2);
  shape.lineTo(-w / 2 + rx, h / 2);
  shape.quadraticCurveTo(-w / 2, h / 2, -w / 2, h / 2 - ry);
  shape.lineTo(-w / 2, -h / 2 + ry);
  shape.quadraticCurveTo(-w / 2, -h / 2, -w / 2 + rx, -h / 2);
  const geometry = new THREE.ShapeGeometry(shape, 6);
  const uv = geometry.getAttribute("uv");
  const position = geometry.getAttribute("position");
  for (let index = 0; index < uv.count; index += 1) {
    uv.setXY(
      index,
      position.getX(index) / w + 0.5,
      position.getY(index) / h + 0.5,
    );
  }
  return geometry;
}
