import * as THREE from "three";

/** The existing printed panel stays fixed while its margins taper into a welded seal. */
export type SleeveFilmProfile = {
  panelScaleX: number;
  panelScaleY: number;
  panelOffsetY: number;
  backZ: number;
  frontZ: number;
  sealThickness: number;
  openingY: number;
};

/** Build curved film shoulders and a thin side/bottom seal, leaving the top open. */
export function createPressedSleeveSeam(
  closedContour: readonly THREE.Vector2[],
  profile: SleeveFilmProfile,
): THREE.BufferGeometry {
  const contour = closedContour.slice(0, -1);
  const count = contour.length;
  const vertices: number[] = [];
  const indices: number[] = [];
  const rings = [0, 0.16, 0.36, 0.58, 0.8, 1];
  const middleZ = (profile.frontZ + profile.backZ) / 2;

  for (const panelZ of [profile.frontZ, profile.backZ]) {
    const sideStart = vertices.length / 3;
    const isFront = panelZ === profile.frontZ;
    const sealZ = middleZ + ((isFront ? 1 : -1) * profile.sealThickness) / 2;
    for (const t of rings) {
      // Finish the rounded shoulder before the outermost ring, leaving a flat weld band.
      const shoulder = Math.min(t / 0.8, 1);
      const ease = shoulder * shoulder * (3 - 2 * shoulder);
      for (const point of contour) {
        const innerX = point.x * profile.panelScaleX;
        const innerY = point.y * profile.panelScaleY + profile.panelOffsetY;
        vertices.push(
          THREE.MathUtils.lerp(innerX, point.x, t),
          THREE.MathUtils.lerp(innerY, point.y, t),
          THREE.MathUtils.lerp(panelZ, sealZ, ease),
        );
      }
    }
    for (let ring = 0; ring < rings.length - 1; ring++) {
      for (let i = 0; i < count; i++) {
        const next = (i + 1) % count;
        const a = contour[i],
          b = contour[next];
        if (!a || !b || (a.y >= profile.openingY && b.y >= profile.openingY))
          continue;
        const innerA = sideStart + ring * count + i;
        const innerB = sideStart + ring * count + next;
        const outerA = innerA + count,
          outerB = innerB + count;
        if (isFront)
          indices.push(innerA, outerA, innerB, innerB, outerA, outerB);
        else indices.push(innerA, innerB, outerA, innerB, outerB, outerA);
      }
    }
  }

  // The weld has a real, small thickness rather than two panels meeting at a knife edge.
  for (let i = 0; i < count; i++) {
    const a = contour[i],
      b = contour[(i + 1) % count];
    if (!a || !b || (a.y >= profile.openingY && b.y >= profile.openingY))
      continue;
    const base = vertices.length / 3;
    const back = middleZ - profile.sealThickness / 2;
    const front = middleZ + profile.sealThickness / 2;
    vertices.push(
      a.x,
      a.y,
      back,
      b.x,
      b.y,
      back,
      a.x,
      a.y,
      front,
      b.x,
      b.y,
      front,
    );
    indices.push(base, base + 1, base + 2, base + 2, base + 1, base + 3);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute(
    "position",
    new THREE.Float32BufferAttribute(vertices, 3),
  );
  geometry.setIndex(indices);
  geometry.computeVertexNormals();
  return geometry;
}
