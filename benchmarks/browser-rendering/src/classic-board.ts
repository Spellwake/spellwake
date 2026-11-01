/** Selected free Classic composition. Coordinates are always relative to the owner. */
import * as THREE from "three";
import { librarySamples } from "./pile-browser";

export const BOARD_SAMPLES = [
  ["llanowar-elves", "Llanowar Elves"],
  ["runeclaw-bear", "Runeclaw Bear"],
  ["serra-angel", "Serra Angel"],
  ["chandra-nalaar", "Chandra Nalaar"],
  ["crystal-ball", "Crystal Ball"],
  ["pacifism", "Pacifism"],
  ["krenko", "Krenko, Mob Boss"],
] as const;

/** Reuse full card/deck meshes; the spatial mock has no playmat or inferred rules state. */
export function createClassicBoard(
  template: THREE.Group,
  land: THREE.Material,
  materials: ReadonlyMap<string, THREE.Material>,
  surface: number,
): THREE.Group {
  const group = new THREE.Group();
  const source = template.children.find(
    (child) =>
      child.userData.isStudyCard === true &&
      !(child instanceof THREE.InstancedMesh),
  );
  if (!source) return group;
  const addCard = (key: string, x: number, z: number, tapped = false): void => {
    const card = source.clone();
    card.position.set(x, surface + 0.035, z);
    card.scale.setScalar(0.68);
    card.rotation.set(-Math.PI / 2, 0, tapped ? Math.PI / 2 : 0);
    card.userData.faceDown = false;
    card.userData.studyLand = key === "land";
    card.userData.studyFace = key;
    card.traverse((part) => {
      if (part instanceof THREE.Mesh && part.userData.printedFront === true)
        part.material =
          key === "land" ? land : (materials.get(key) ?? part.material);
    });
    group.add(card);
  };
  // Commander presentation lives beside the hand; these are the physical board piles.
  addCard("pacifism", -6.7, -0.2);
  const exile = group.children.at(-1);
  if (exile) {
    exile.userData.pileKind = "exile";
    exile.userData.pileEntries = ["pacifism", "crystal-ball", "lightning-bolt"];
  }
  addCard("serra-angel", -6.7, 2.8);
  const graveyard = group.children.at(-1);
  if (graveyard) {
    graveyard.userData.pileKind = "graveyard";
    graveyard.userData.pileEntries = [
      "serra-angel",
      "runeclaw-bear",
      "llanowar-elves",
    ];
  }

  const deck = new THREE.Group();
  // First five source children are loose study cards; the rest are the complete deck.
  for (const part of template.children.slice(5)) deck.add(part.clone());
  deck.scale.set(0.68, 1, 0.68);
  deck.position.set(-4.6 - 5.05 * 0.68, 0, 2.8 + 1.9 * 0.68);
  deck.userData.pileKind = "library";
  deck.userData.pileCount = template.children
    .slice(5)
    .reduce(
      (count, child) =>
        count + (child instanceof THREE.InstancedMesh ? child.count : 1),
      0,
    );
  deck.userData.pileCards = librarySamples(Number(deck.userData.pileCount));
  group.add(deck);

  for (const [i, key] of [
    "llanowar-elves",
    "runeclaw-bear",
    "serra-angel",
  ].entries())
    addCard(key, -1.8 + i * 2, -0.2);
  for (let i = 0; i < 5; i++) {
    addCard("land", -2.1 + i * 1.1, 2.8, i === 4);
    const card = group.children.at(-1);
    if (card) card.position.y += i * 0.025;
  }
  addCard("chandra-nalaar", 6.1, -0.2);
  addCard("crystal-ball", 5.1, 2.8);
  addCard("pacifism", 7.1, 2.8);
  return group;
}
