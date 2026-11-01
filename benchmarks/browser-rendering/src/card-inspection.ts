/** Move a card from its existing scene pose into reading distance, then back home. */
import * as THREE from "three";

export function createCardInspection(camera: THREE.PerspectiveCamera) {
  let label = "card";
  let flight: {
    source: THREE.Object3D;
    copy: THREE.Object3D;
    closing: boolean;
  } | null = null;
  const matrix = new THREE.Matrix4(),
    position = new THREE.Vector3(),
    scale = new THREE.Vector3(),
    rotation = new THREE.Quaternion();
  function sourcePose(source: THREE.Object3D): void {
    source.updateWorldMatrix(true, false);
    matrix
      .multiplyMatrices(camera.matrixWorldInverse, source.matrixWorld)
      .decompose(position, rotation, scale);
  }
  function cancel(): void {
    if (!flight) return;
    flight.source.visible = true;
    flight.copy.removeFromParent();
    flight = null;
  }
  function open(source: THREE.Object3D): void {
    cancel();
    sourcePose(source);
    label = source.userData.faceDown === true ? "card back" : "card";
    if (source.userData.faceDown !== true)
      source.traverse((part) => {
        if (
          part instanceof THREE.Mesh &&
          part.userData.printedFront === true &&
          !Array.isArray(part.material)
        )
          label = part.material.name || "card";
      });
    const copy = source.clone();
    if (typeof source.userData.sourceZone === "string")
      label += ` from ${source.userData.sourceZone === "command" ? "the command zone" : source.userData.sourceZone}`;
    copy.visible = true;
    copy.position.copy(position);
    copy.quaternion.copy(rotation);
    copy.scale.copy(scale);
    copy.traverse((part) => {
      part.castShadow = false;
    });
    camera.add(copy);
    source.visible = false;
    flight = { source, copy, closing: false };
  }
  function close(): void {
    if (flight) flight.closing = true;
  }
  function tick(
    elapsed: number,
    reduced: boolean,
    bottomReservation = 0,
  ): boolean {
    if (!flight) return false;
    if (flight.closing) sourcePose(flight.source);
    else {
      const halfHeight = Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * 5;
      position.set(0, halfHeight * bottomReservation, -5);
      rotation.setFromAxisAngle(
        new THREE.Vector3(0, 1, 0),
        flight.source.userData.faceDown === true ? Math.PI : 0,
      );
      scale.setScalar(
        Math.min(
          (halfHeight * 2 * (1 - bottomReservation) * 0.84) / 3.8,
          (halfHeight * 2 * camera.aspect * 0.82) / 2.8,
        ),
      );
    }
    const blend = reduced ? 1 : 1 - Math.exp(-elapsed * 13);
    flight.copy.position.lerp(position, blend);
    flight.copy.quaternion.slerp(rotation, blend);
    flight.copy.scale.lerp(scale, blend);
    const error =
      flight.copy.position.distanceTo(position) +
      flight.copy.scale.distanceTo(scale) +
      flight.copy.quaternion.angleTo(rotation);
    if (error < 0.001) {
      flight.copy.position.copy(position);
      flight.copy.quaternion.copy(rotation);
      flight.copy.scale.copy(scale);
      if (flight.closing) cancel();
      return false;
    }
    return true;
  }
  return {
    open,
    close,
    cancel,
    tick,
    get label(): string {
      return label;
    },
    get active(): boolean {
      return flight !== null;
    },
  };
}
