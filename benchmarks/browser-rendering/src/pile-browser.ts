/** Browse the upper surfaces of an ordered pile without turning hidden cards over. */
import * as THREE from "three";
import { z } from "zod";
import {
  createFanFocus,
  fanIndexAt,
  fanDepth,
  fanThickness,
} from "./compact-fan";
import { foldedFan, type FoldedFanPose } from "./folded-fan";
import { createCardInspection } from "./card-inspection";

export const pileEntrySchema = z.object({
  key: z.string().nullable(),
  faceUp: z.boolean(),
});
export type PileEntry = z.infer<typeof pileEntrySchema>;
/** Face-down entries carry no identity; the third face-up card is an explicit study sample. */
export function librarySamples(count: number): PileEntry[] {
  return Array.from({ length: count }, (_, i) => ({
    key: i === 2 ? "serra-angel" : null,
    faceUp: i === 2,
  }));
}
export function pileCard(
  template: THREE.Object3D,
  entry: PileEntry,
  materials: ReadonlyMap<string, THREE.Material>,
): THREE.Object3D {
  const copy = template.clone(),
    material = entry.key ? materials.get(entry.key) : undefined;
  const faceUp = entry.faceUp && material !== undefined;
  copy.visible = true;
  copy.userData.faceDown = !faceUp;
  copy.userData.isHandCard = false;
  const remove: THREE.Object3D[] = [];
  copy.traverse((part) => {
    part.castShadow = false;
    if (part instanceof THREE.Mesh && part.userData.printedFront === true) {
      if (faceUp && material) part.material = material;
      else remove.push(part);
    }
  });
  for (const front of remove) front.removeFromParent();
  copy.rotation.set(0, faceUp ? 0 : Math.PI, 0);
  return copy;
}

export function createPileBrowser(
  camera: THREE.PerspectiveCamera,
  inspection: ReturnType<typeof createCardInspection>,
  template: () => THREE.Object3D | undefined,
  materials: ReadonlyMap<string, THREE.Material>,
  changed: () => void,
  host?: HTMLElement,
) {
  const group = new THREE.Group();
  const veil = new THREE.Mesh(
    new THREE.PlaneGeometry(1, 1),
    new THREE.MeshBasicMaterial({
      color: 0x080b09,
      transparent: true,
      opacity: 0.48,
      depthWrite: false,
      toneMapped: false,
    }),
  );
  veil.position.z = -7.6;
  veil.visible = false;
  camera.add(group, veil);
  group.position.set(0, 0.15, -7);
  group.visible = false;
  const scroll = createFanFocus();
  const control = host?.ownerDocument.createElement("input");
  if (control && host) {
    control.type = "range";
    control.min = "0";
    control.step = "1";
    control.hidden = true;
    control.style.cssText =
      "position:absolute;left:50%;top:76%;transform:translateX(-50%);width:min(48vw,420px);height:18px;margin:0;accent-color:#c6b17e;opacity:.72;z-index:2;cursor:default";
    control.addEventListener("input", () => {
      hovered = -1;
      scroll.reset(control.valueAsNumber);
      changed();
    });
    for (const event of ["pointerdown", "pointermove", "pointerup", "wheel"])
      control.addEventListener(event, (input) => input.stopPropagation());
    control.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        readSelected();
        changed();
      }
      if (event.key !== "Escape") event.stopPropagation();
    });
    host.append(control);
  }
  let entries: PileEntry[] = [],
    hovered = -1,
    active = false,
    stageWidth = 800,
    kind = "pile",
    scale = 0.5,
    span = 8;
  let poses: FoldedFanPose[] = [];
  const target = new THREE.Vector3();
  const positioned = new WeakSet<THREE.Object3D>();
  function move(amount: number): void {
    hovered = -1;
    scroll.move(amount);
    changed();
  }
  function open(pile: THREE.Object3D): void {
    kind = z
      .enum(["library", "graveyard", "exile", "command"])
      .parse(pile.userData.pileKind);
    inspection.cancel();
    group.clear();
    entries =
      pile.userData.pileCards !== undefined
        ? z.array(pileEntrySchema).parse(pile.userData.pileCards)
        : z
            .array(z.string())
            .parse(pile.userData.pileEntries)
            .map((key) => ({ key, faceUp: true }));
    scroll.configure(entries.length);
    scroll.reset();
    hovered = -1;
    active = true;
    group.visible = true;
    veil.visible = true;
    if (control) {
      control.max = String(Math.max(0, entries.length - 1));
      control.value = "0";
      control.setAttribute("aria-label", `Browse ${kind}`);
    }
    const original = template();
    if (original)
      for (const [i, entry] of entries.entries()) {
        const card = pileCard(original, entry, materials);
        card.userData.pileIndex = i;
        group.add(card);
      }
    changed();
  }
  function close(): void {
    inspection.cancel();
    active = false;
    group.visible = false;
    veil.visible = false;
    group.clear();
    entries = [];
    scroll.reset();
    if (control) control.hidden = true;
  }
  function hit(ray: THREE.Raycaster): THREE.Object3D | null {
    const localRay = ray.ray.clone().applyMatrix4(camera.matrixWorldInverse);
    const distance = (-7 - localRay.origin.z) / localRay.direction.z;
    if (distance < 0) return null;
    const point = localRay.at(distance, new THREE.Vector3());
    if (
      Math.abs(point.x) > ((span + 2.5) * scale) / 2 ||
      Math.abs(point.y - group.position.y) > 2.1 * scale + 0.3
    )
      return null;
    const readable = poses.flatMap((pose, index) =>
      pose.folded ? [] : [{ pose, index }],
    );
    const slot = fanIndexAt(
      point.x / scale,
      readable.map((entry) => entry.pose),
      1.25,
    );
    return group.children[readable[slot]?.index ?? 0] ?? null;
  }
  function read(ray: THREE.Raycaster): boolean {
    const card = hit(ray);
    if (!card) return false;
    scroll.move(0);
    inspection.open(card);
    return true;
  }
  function hover(ray: THREE.Raycaster): boolean {
    const index = group.children.indexOf(hit(ray) ?? group);
    if (index === hovered) return false;
    hovered = index;
    return true;
  }
  function frame(width: number, elapsed: number, reduced: boolean): boolean {
    if (!active) return false;
    stageWidth = width;
    scroll.configure(entries.length);
    let moving = scroll.tick(elapsed, reduced);
    const count = entries.length;
    if (control) {
      control.hidden = inspection.active || entries.length <= 7;
      control.value = String(Math.round(scroll.position));
      control.setAttribute(
        "aria-valuetext",
        `Card ${Math.round(scroll.position) + 1} of ${entries.length}`,
      );
    }
    const halfHeight = Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * 7;
    const veilHeight =
      Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * 7.6 * 2;
    veil.scale.set(veilHeight * camera.aspect, veilHeight, 1);
    group.position.y = 0.45;
    span = count > 7 ? 8.3 : Math.max(0, count - 1) * 1.1;
    scale = Math.min(
      0.72,
      (halfHeight * 2 * 0.58) / 3.8,
      (halfHeight * 2 * camera.aspect * 0.88) / (span + 3.0),
    );
    poses = foldedFan(count, scroll.position);
    const blend = reduced ? 1 : 1 - Math.exp(-elapsed * 22);
    const selected = hovered;
    for (const [i, card] of group.children.entries()) {
      const pose = poses[i];
      if (!inspection.active) card.visible = true;
      if (!pose) continue;
      const activeCard = i === selected;
      const amount = positioned.has(card) ? blend : 1;
      target.set(
        pose.x * scale,
        pose.y * scale + (activeCard ? 0.3 : 0),
        fanDepth(pose.depth, count, scale, activeCard),
      );
      card.position.lerp(target, amount);
      card.rotation.z = THREE.MathUtils.lerp(
        card.rotation.z,
        activeCard
          ? 0
          : pose.angle * (card.userData.faceDown === true ? -1 : 1),
        amount,
      );
      const size = scale;
      const depthScale = size * fanThickness(count);
      card.scale.lerp(new THREE.Vector3(size, size, depthScale), amount);
      positioned.add(card);
      moving ||=
        card.position.distanceTo(target) +
          Math.abs(card.scale.x - size) +
          Math.abs(card.scale.z - depthScale) >
        0.001;
    }
    return moving;
  }
  function readSelected(): void {
    const card =
      group.children[hovered >= 0 ? hovered : Math.round(scroll.position)];
    if (card) {
      scroll.move(0);
      inspection.open(card);
    }
  }
  return {
    dispose(): void {
      control?.remove();
    },
    open,
    close,
    frame,
    read,
    hover,
    move,
    beginPan(): void {
      hovered = -1;
      scroll.begin();
    },
    pan(pixels: number, seconds: number): void {
      scroll.pan(
        (pixels * Math.max(8, entries.length * 0.5)) / stageWidth,
        seconds,
      );
      changed();
    },
    endPan(paused: boolean): void {
      scroll.end(paused);
      changed();
    },
    readSelected,
    get description(): string {
      return `Browsing ${kind}: ${entries.length} ${entries.length === 1 ? "card" : "cards"} in an overlapping fan. Drag sideways or use the browsing control; Enter reads, Escape returns.`;
    },
    get active(): boolean {
      return active;
    },
    get count(): number {
      return entries.length;
    },
  };
}
