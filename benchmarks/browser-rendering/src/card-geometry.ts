/** Approved rounded Magic card, pressed soft-sleeve, and instanced deck-edge geometries. */
import * as THREE from "three";
import { createPressedSleeveSeam } from "./sleeve-profile";

/** Standard card dimensions in scene units (2.5 x 3.49 in proportion). */
export const CARD_WIDTH = 2.5;
export const CARD_HEIGHT = 3.49;
export const CARD_CORNER_RADIUS = 0.12;
/** Approved thinner card & soft-sleeve depth scale (Oct 31, 2026). */
export const CARD_THICKNESS_SCALE = 0.6;

/** Soft sleeve outer dimensions in scene units. */
export const SLEEVE_WIDTH = 2.62;
export const SLEEVE_HEIGHT = 3.65;
export const SLEEVE_CORNER_RADIUS = 0.11;

/** Rounded rectangle silhouette in the XY plane. */
export function createCardSilhouette(
  width = CARD_WIDTH,
  height = CARD_HEIGHT,
  radius = CARD_CORNER_RADIUS,
): THREE.Shape {
  const x = -width / 2;
  const y = -height / 2;
  const shape = new THREE.Shape();
  shape.moveTo(x + radius, y);
  shape.lineTo(x + width - radius, y);
  shape.quadraticCurveTo(x + width, y, x + width, y + radius);
  shape.lineTo(x + width, y + height - radius);
  shape.quadraticCurveTo(x + width, y + height, x + width - radius, y + height);
  shape.lineTo(x + radius, y + height);
  shape.quadraticCurveTo(x, y + height, x, y + height - radius);
  shape.lineTo(x, y + radius);
  shape.quadraticCurveTo(x, y, x + radius, y);
  return shape;
}

/** Map a rounded face cleanly to full [0..1] UVs without stretching margins. */
export function createMappedFaceGeometry(
  shape: THREE.Shape,
  width: number,
  height: number,
  curveSegments = 14,
): THREE.ShapeGeometry {
  const geometry = new THREE.ShapeGeometry(shape, curveSegments);
  const positions = geometry.getAttribute("position");
  const uv = geometry.getAttribute("uv");
  for (let i = 0; i < positions.count; i++) {
    uv.setXY(
      i,
      positions.getX(i) / width + 0.5,
      positions.getY(i) / height + 0.5,
    );
  }
  return geometry;
}

/** Rounded extruded cardstock geometry at the approved thickness scale. */
export function createCardStockGeometry(
  shape: THREE.Shape = createCardSilhouette(),
): THREE.ExtrudeGeometry {
  const geometry = new THREE.ExtrudeGeometry(shape, {
    depth: 0.012,
    bevelEnabled: true,
    bevelSize: 0.002,
    bevelThickness: 0.002,
    bevelSegments: 2,
    steps: 1,
    curveSegments: 12,
  });
  geometry.translate(0, 0, -0.006);
  geometry.scale(1, 1, CARD_THICKNESS_SCALE);
  return geometry;
}

/** Shared pressed-edge soft sleeve geometry set (approved Oct 31, 2026). */
export function createSoftSleeveGeometries(): {
  backFace: THREE.ShapeGeometry;
  stock: THREE.ExtrudeGeometry;
  seam: THREE.BufferGeometry;
  frontFilm: THREE.ShapeGeometry;
} {
  const sleeveShape = createCardSilhouette(
    SLEEVE_WIDTH,
    SLEEVE_HEIGHT,
    SLEEVE_CORNER_RADIUS,
  );
  const backFace = createMappedFaceGeometry(
    sleeveShape,
    SLEEVE_WIDTH,
    SLEEVE_HEIGHT,
    14,
  );
  const stock = new THREE.ExtrudeGeometry(sleeveShape, {
    depth: 0.008,
    bevelEnabled: false,
    curveSegments: 12,
    steps: 1,
  });
  stock.translate(0, 0, -0.026);
  stock.scale(1, 1, CARD_THICKNESS_SCALE);

  const seam = createPressedSleeveSeam(sleeveShape.getPoints(20), {
    panelScaleX: 0.94,
    panelScaleY: 0.955,
    panelOffsetY: 0.02,
    backZ: -0.026 * CARD_THICKNESS_SCALE,
    frontZ: 0.024 * CARD_THICKNESS_SCALE,
    sealThickness: 0.002 * CARD_THICKNESS_SCALE,
    openingY: SLEEVE_HEIGHT / 2 - 0.01,
  });

  const frontFilm = createMappedFaceGeometry(
    sleeveShape,
    SLEEVE_WIDTH,
    SLEEVE_HEIGHT,
    12,
  );
  return { backFace, stock, seam, frontFilm };
}

/** Thin rounded deck-layer plate for instanced library/graveyard stacks. */
export function createDeckLayerGeometry(): THREE.ExtrudeGeometry {
  const shape = createCardSilhouette(
    SLEEVE_WIDTH,
    SLEEVE_HEIGHT,
    SLEEVE_CORNER_RADIUS,
  );
  const geometry = new THREE.ExtrudeGeometry(shape, {
    depth: 0.015,
    bevelEnabled: false,
    curveSegments: 8,
    steps: 1,
  });
  geometry.translate(0, 0, -0.0075);
  return geometry;
}

/** Slightly expanded rounded frame for glowing 3D playable/target/combat rims. */
export function createRimGeometry(padding = 0.14): THREE.ShapeGeometry {
  const outer = createCardSilhouette(
    SLEEVE_WIDTH + padding * 2,
    SLEEVE_HEIGHT + padding * 2,
    SLEEVE_CORNER_RADIUS + padding * 0.65,
  );
  const inner = createCardSilhouette(
    SLEEVE_WIDTH - 0.03,
    SLEEVE_HEIGHT - 0.03,
    SLEEVE_CORNER_RADIUS,
  );
  outer.holes.push(new THREE.Path(inner.getPoints(12)));
  return new THREE.ShapeGeometry(outer, 12);
}
