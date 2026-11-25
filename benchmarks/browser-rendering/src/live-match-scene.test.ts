import { describe, expect, test } from "bun:test";
import * as THREE from "three";
import {
  CARD_WIDTH,
  createCardSilhouette,
  createCardStockGeometry,
  createDeckLayerGeometry,
  createMappedFaceGeometry,
  createRimGeometry,
  createSoftSleeveGeometries,
} from "./card-geometry";
import {
  cardTargetQuaternion,
  stageToHandLocal,
  stageToTableWorld,
  tableCameraDistance,
} from "./live-match-scene";

describe("live-match-scene geometry and 3D projection helpers", () => {
  test("approved card, pressed sleeve, rim, and deck geometries construct finite vertices", () => {
    const shape = createCardSilhouette();
    const face = createMappedFaceGeometry(shape, CARD_WIDTH, 3.49);
    const stock = createCardStockGeometry(shape);
    const sleeve = createSoftSleeveGeometries();
    const layer = createDeckLayerGeometry();
    const rim = createRimGeometry();

    for (const geo of [
      face,
      stock,
      sleeve.backFace,
      sleeve.stock,
      sleeve.seam,
      sleeve.frontFilm,
      layer,
      rim,
    ]) {
      const pos = geo.getAttribute("position");
      expect(pos.count).toBeGreaterThan(0);
      for (let i = 0; i < pos.count; i++) {
        expect(Number.isFinite(pos.getX(i))).toBe(true);
        expect(Number.isFinite(pos.getY(i))).toBe(true);
        expect(Number.isFinite(pos.getZ(i))).toBe(true);
      }
      geo.dispose();
    }
  });

  test("stageToTableWorld maps stage center to origin and preserves proportional scale", () => {
    const center = stageToTableWorld(720, 450, 120, { width: 1440, height: 900 });
    expect(center.x).toBeCloseTo(0, 5);
    expect(center.z).toBeCloseTo(0, 5);
    expect(center.scale).toBeGreaterThan(0);

    const left = stageToTableWorld(0, 0, 120, { width: 1440, height: 900 });
    expect(left.x).toBeLessThan(0);
    expect(left.z).toBeLessThan(0);
  });

  test("stageToHandLocal maps bottom-center into negative-Y camera space", () => {
    const hand = stageToHandLocal(720, 820, 160, { width: 1440, height: 900 });
    expect(hand.x).toBeCloseTo(0, 5);
    expect(hand.y).toBeLessThan(0);
    expect(hand.scale).toBeGreaterThan(0);
    expect(tableCameraDistance()).toBeGreaterThan(40);
  });

  test("cardTargetQuaternion lays battlefield cards face-up, flips face-down, and rotates 90deg clockwise when tapped", () => {
    const up = cardTargetQuaternion("battlefield", false, 0);
    const frontNormal = new THREE.Vector3(0, 0, 1).applyQuaternion(up);
    const topDir = new THREE.Vector3(0, 1, 0).applyQuaternion(up);
    expect(frontNormal.y).toBeCloseTo(1, 4);
    expect(topDir.z).toBeCloseTo(-1, 4);

    const tapped = cardTargetQuaternion("battlefield", false, 90);
    const tappedTop = new THREE.Vector3(0, 1, 0).applyQuaternion(tapped);
    expect(tappedTop.x).toBeCloseTo(1, 4);

    const faceDown = cardTargetQuaternion("pile", true, 0);
    const backNormal = new THREE.Vector3(0, 0, -1).applyQuaternion(faceDown);
    const backTop = new THREE.Vector3(0, 1, 0).applyQuaternion(faceDown);
    expect(backNormal.y).toBeCloseTo(1, 4);
    expect(backTop.z).toBeCloseTo(-1, 4);
  });
});
