/** Visual cast/resolve stages only; no legality, priority, targets or payment are simulated. */
import * as THREE from "three";

type Flight = {
  card: THREE.Object3D;
  position: THREE.Vector3;
  rotation: THREE.Quaternion;
  scale: number;
  ready: boolean;
};
type Spell = { flight: Flight; board: THREE.Object3D; surface: number };
export function createPlayPreview(
  scene: THREE.Scene,
  camera: THREE.PerspectiveCamera,
) {
  const group = new THREE.Group();
  scene.add(group);
  const flights: Flight[] = [],
    stack: Spell[] = [];
  const rotation = new THREE.Quaternion().setFromEuler(
    new THREE.Euler(-Math.PI / 2, 0, 0),
  );
  function clear(): void {
    group.clear();
    flights.length = 0;
    stack.length = 0;
  }
  function drop(
    card: THREE.Object3D,
    board: THREE.Object3D,
    surface: number,
  ): void {
    card.updateWorldMatrix(true, false);
    group.attach(card);
    card.userData.isHandCard = false;
    const land = card.userData.studyLand === true;
    const flight: Flight = {
      card,
      ready: false,
      rotation: rotation.clone(),
      scale: 0.68,
      position: land
        ? board.localToWorld(new THREE.Vector3(0, surface + 0.3, 2.8))
        : new THREE.Vector3(
            stack.length * 0.35,
            surface + 0.4 + stack.length * 0.08,
            0,
          ),
    };
    flights.push(flight);
    if (!land) stack.push({ flight, board, surface });
  }
  /** Explicit demo control stands in for engine-authorized resolution, never a timer. */
  function resolveTop(): boolean {
    const spell = stack.at(-1);
    if (!spell?.flight.ready) return false;
    stack.pop();
    spell.flight.position.copy(
      spell.board.localToWorld(
        new THREE.Vector3(
          -6.7,
          spell.surface + 0.3 + flights.length * 0.025,
          2.8,
        ),
      ),
    );
    spell.flight.ready = false;
    spell.flight.rotation.copy(rotation);
    spell.flight.scale = 0.68;
    // The sample hand contains Lightning Bolt instants. Permanent resolution is not modeled here.
    const entries: string[] = [];
    let found = false;
    spell.board.traverse((part) => {
      if (
        !found &&
        part.userData.pileKind === "graveyard" &&
        Array.isArray(part.userData.pileEntries)
      ) {
        found = true;
        for (const key of part.userData.pileEntries)
          if (typeof key === "string") entries.push(key);
      }
    });
    const key =
      typeof spell.flight.card.userData.studyFace === "string"
        ? spell.flight.card.userData.studyFace
        : "lightning-bolt";
    entries.unshift(key);
    spell.board.traverse((part) => {
      if (part.userData.pileKind === "graveyard")
        part.userData.pileEntries = entries;
    });
    group.traverse((part) => {
      if (part.userData.pileOwner === spell.board.uuid)
        part.userData.pileEntries = entries;
    });
    spell.flight.card.userData.pileOwner = spell.board.uuid;
    spell.flight.card.userData.pileKind = "graveyard";
    spell.flight.card.userData.pileEntries = entries;
    return true;
  }
  function tick(enabled: boolean, elapsed: number, reduced: boolean): boolean {
    group.visible = enabled;
    let moving = false;
    camera.updateWorldMatrix(true, false);
    const halfHeight = Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * 6.5;
    const hoverScale = Math.min(
      0.95,
      (halfHeight * 2 * 0.48) / 3.5,
      (halfHeight * 2 * camera.aspect * 0.62) / 2.5,
    );
    for (const [i, spell] of stack.entries()) {
      // Pending spells stay suspended in front of the viewer, independent of table zoom.
      spell.flight.position
        .set(i * 0.24, 0.2 + i * 0.1, -6.5 - i * 0.04)
        .applyMatrix4(camera.matrixWorld);
      spell.flight.rotation
        .copy(camera.quaternion)
        .multiply(
          new THREE.Quaternion().setFromEuler(new THREE.Euler(-0.1, -0.08, 0)),
        );
      spell.flight.scale = hoverScale;
    }
    for (const flight of flights) {
      const blend = reduced ? 1 : 1 - Math.exp(-elapsed * 13);
      flight.card.position.lerp(flight.position, blend);
      flight.card.quaternion.slerp(flight.rotation, blend);
      flight.card.scale.lerp(
        new THREE.Vector3(flight.scale, flight.scale, flight.scale),
        blend,
      );
      flight.ready =
        flight.card.position.distanceTo(flight.position) +
          flight.card.quaternion.angleTo(flight.rotation) +
          Math.abs(flight.card.scale.x - flight.scale) <
        0.001;
      moving ||= !flight.ready;
    }
    return moving;
  }
  return {
    group,
    drop,
    tick,
    clear,
    resolveTop,
    get count(): number {
      return stack.length;
    },
    get canResolve(): boolean {
      return stack.at(-1)?.flight.ready === true;
    },
  };
}
