/** Isolated camera hand and back-only opponent fans, outside match state. */
import * as THREE from "three";
import { pileCard, type PileEntry } from "./pile-browser";
import { foldedFan } from "./folded-fan";
/** A fresh upward segment can lift after any length of horizontal browsing. */
export function isHandLift(
  pressY: number,
  previousX: number,
  previousY: number,
  x: number,
  y: number,
): boolean {
  return pressY - y > 18 && previousY - y > Math.abs(x - previousX);
}
export type PublicHandEntry = PileEntry & {
  canPlay?: boolean | undefined;
  sourceId?: string | undefined;
};
export type AvailableCard = PublicHandEntry & {
  sourceZone: "command" | "exile" | "graveyard" | "library" | "outside-game";
};
export type PublicPlayerZones = {
  hand?: readonly PublicHandEntry[] | undefined;
  command?: readonly PileEntry[] | undefined;
  availableCards?: readonly AvailableCard[] | undefined;
};
import {
  groupedHandFan,
  fanDepth,
  fanThickness,
  createFanFocus,
  fanIndexAt,
  fanWindow,
  type FanPose,
} from "./compact-fan";

export function createHandStudy(
  camera: THREE.PerspectiveCamera,
  landMaterial: THREE.Material,
  commanderMaterial?: THREE.Material,
  faceMaterials: ReadonlyMap<string, THREE.Material> = new Map(),
  options: {
    host?: HTMLElement;
    fixture?: boolean;
    changed?: () => void;
    read?: (card: THREE.Object3D) => void;
  } = {},
) {
  const { host } = options;
  const local = new THREE.Group(),
    opponents = new THREE.Group(),
    external = new THREE.Group(),
    opponentExternal = new THREE.Group();
  const viewedHand = new THREE.Group(),
    viewedExternal = new THREE.Group();
  camera.add(local, external, viewedHand, viewedExternal);
  let sourceTemplate: THREE.Object3D | null = null;
  let playerZones: readonly PublicPlayerZones[] = [];
  let handCounts: readonly number[] = [];
  let viewedSeat = 0;
  let viewerSeat = 1;
  let wasEnabled = false;
  let entranceTime = 1;
  const commanderAuraGeometry = new THREE.PlaneGeometry(2.95, 3.95);
  const commanderAuraMaterial = new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    toneMapped: false,
    vertexShader: `varying vec2 vUv; void main(){vUv=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);}`,
    fragmentShader: `varying vec2 vUv;
      void main(){
        vec2 p=(vUv-.5)*vec2(2.95,3.95);
        vec2 q=abs(p)-vec2(1.13,1.625);
        float d=length(max(q,0.0))+min(max(q.x,q.y),0.0)-.12;
        float core=exp(-pow((d-.025)/.018,2.0));
        float halo=exp(-max(d,0.0)*18.0)*smoothstep(-.005,.012,d);
        float alpha=core*.85+halo*.32;
        gl_FragColor=vec4(mix(vec3(.18,.72,.72),vec3(1.0,.79,.38),core),alpha);
      }`,
  });
  function viewOpponent(seat: number): void {
    if (seat === viewedSeat) return;
    wasEnabled = false;
    viewedSeat = seat;
    viewedHand.clear();
    viewedExternal.clear();
    selected = -1;
    dragged = null;
    panTarget = null;
    focusStates.delete(viewedHand);
    focusStates.delete(viewedExternal);
    if (!seat || !sourceTemplate) return;
    const zones = playerZones[seat - 1];
    const entries =
      zones?.hand ??
      Array.from({ length: handCounts[seat - 1] ?? 0 }, () => ({
        key: null,
        faceUp: false,
      }));
    const add = (group: THREE.Group, entry: PublicHandEntry): void => {
      if (!sourceTemplate) return;
      const card = pileCard(sourceTemplate, entry, faceMaterials);
      card.position.set(0, 0, 0);
      card.scale.setScalar(1);
      card.userData.isStudyCard = true;
      card.userData.isHandCard = false;
      card.userData.canPlay = entry.canPlay === true && !card.userData.faceDown;
      if (entry.sourceId && entry.faceUp)
        card.userData.sourceId = entry.sourceId;
      delete card.userData.studyFace;
      delete card.userData.studyLand;
      delete card.userData.pileKind;
      if (!card.userData.faceDown) card.userData.studyFace = entry.key;
      group.add(card);
    };
    for (const entry of entries) add(viewedHand, entry);
    selected = -1;
    dragged = null;

    for (const entry of availableForSeat(seat))
      viewedExternal.add(externalCard(sourceTemplate, entry));
  }
  type DisplayPose = FanPose & { depth: number; folded?: boolean };
  type FocusState = ReturnType<typeof createFanFocus>;
  type PanTarget = {
    group: THREE.Group;
    key: string;
    indices: number[];
    offset: number;
    position: number;
  };
  const focusStates = new WeakMap<THREE.Group, Map<string, FocusState>>();
  let panTarget: PanTarget | null = null;
  let browsingBlocked = false;
  const windowHidden = new WeakSet<THREE.Object3D>();
  let poses: (DisplayPose | undefined)[] = [];
  function focusFor(group: THREE.Group, key = "hand"): FocusState {
    let states = focusStates.get(group);
    if (!states) {
      states = new Map();
      focusStates.set(group, states);
    }
    let state = states.get(key);
    if (!state) {
      state = createFanFocus();
      states.set(key, state);
    }
    return state;
  }
  function indicesFor(group: THREE.Group, key: string): number[] {
    return group.children.flatMap((card, index) =>
      key === "hand" || card.userData.sourceZone === key ? [index] : [],
    );
  }
  function groupForIndex(index: number): PanTarget | null {
    const offset =
      index < activeHand().children.length ? 0 : activeHand().children.length;
    const group =
      index < activeHand().children.length ? activeHand() : activeExternal();
    const card = group.children[index - offset];
    if (!card) return null;
    const key =
      group === activeHand() ? "hand" : String(card.userData.sourceZone);
    const indices = indicesFor(group, key);
    return {
      group,
      key,
      indices,
      offset,
      position: indices.indexOf(index - offset),
    };
  }
  const control = host?.ownerDocument.createElement("input");
  if (control) {
    control.type = "range";
    control.min = "0";
    control.step = "1";
    control.hidden = true;
    control.style.cssText =
      "position:absolute;left:50%;bottom:8px;transform:translateX(-50%);width:min(34vw,340px);height:24px;margin:0;accent-color:#c6b17e;opacity:.8;z-index:2;cursor:default";
    control.addEventListener("input", () => {
      if (panTarget) focusFor(panTarget.group, panTarget.key).end(true);
      panTarget = null;
      const state = focusFor(activeHand());
      state.configure(activeHand().children.length);
      state.reset(control.valueAsNumber);
      selected = control.valueAsNumber;
      options.changed?.();
    });
    for (const name of ["pointerdown", "pointermove", "pointerup", "wheel"])
      control.addEventListener(name, (event) => event.stopPropagation());
    control.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        const index =
          selected >= 0
            ? selected
            : Math.round(focusFor(activeHand()).position);
        const card = cardAt(index);
        if (card) options.read?.(card);
      }
      if (event.key !== "Escape") event.stopPropagation();
    });
    host?.append(control);
  }
  function availableForSeat(seat: number): readonly AvailableCard[] {
    const zones = playerZones[seat - 1];
    const entries =
      zones?.availableCards ??
      (
        zones?.command ??
        (commanderMaterial && options.fixture !== false
          ? [{ key: "krenko", faceUp: true }]
          : [])
      ).map((entry) => ({
        ...entry,
        sourceZone: "command" as const,
        canPlay: options.fixture !== false && seat === viewerSeat,
      }));
    const order: readonly AvailableCard["sourceZone"][] = [
      "command",
      "exile",
      "graveyard",
      "library",
      "outside-game",
    ];
    return [...entries].sort(
      (a, b) => order.indexOf(a.sourceZone) - order.indexOf(b.sourceZone),
    );
  }
  function externalCard(
    template: THREE.Object3D,
    entry: AvailableCard,
  ): THREE.Object3D {
    const materials = new Map(faceMaterials);
    if (commanderMaterial && !materials.has("krenko"))
      materials.set("krenko", commanderMaterial);
    const card = pileCard(template, entry, materials);
    card.userData = {
      isStudyCard: true,
      isHandCard: false,
      faceDown: card.userData.faceDown,
      sourceZone: entry.sourceZone,
      canPlay: entry.canPlay === true && card.userData.faceDown !== true,
      ...(entry.sourceId && entry.faceUp ? { sourceId: entry.sourceId } : {}),
    };
    if (!card.userData.faceDown) card.userData.studyFace = entry.key;
    card.userData.studyLand = entry.key === "land";
    if (entry.sourceZone === "command") {
      const aura = new THREE.Mesh(commanderAuraGeometry, commanderAuraMaterial);
      aura.position.z = 0.032;
      aura.userData.commanderAura = true;
      card.add(aura);
    }
    card.position.set(0, 0, 0);
    card.scale.setScalar(1);
    return card;
  }
  let selected = -1,
    scale = 0.5;
  let dragged: THREE.Object3D | null = null;
  function copyCard(template: THREE.Object3D, hidden: boolean): THREE.Object3D {
    const copy = template.clone();
    copy.visible = true;
    copy.userData.isStudyCard = !hidden;
    copy.userData.faceDown = hidden;
    copy.userData.isHandCard = !hidden;
    if (hidden)
      copy.userData = { isStudyCard: false, faceDown: true, isHandCard: false };
    const fronts: THREE.Object3D[] = [];
    copy.traverse((part) => {
      part.castShadow = false;
      if (hidden && part.userData.printedFront === true) fronts.push(part);
    });
    for (const front of fronts) front.removeFromParent();
    return copy;
  }
  /** Rebuild proxies from public counts. Opponent copies omit their printed front. */
  function rebuild(
    template: THREE.Object3D,
    boards: THREE.Group,
    viewer: number,
    ownCount: number,
    opponentCount: number,
    surface: number,
    publicHandCounts?: readonly number[],
    publicZones?: readonly PublicPlayerZones[],
  ): void {
    sourceTemplate = template;
    viewerSeat = viewer;
    playerZones = publicZones ?? [];
    handCounts = boards.children.map(
      (_, index) => publicHandCounts?.[index] ?? opponentCount,
    );
    viewedSeat = -1;
    viewOpponent(0);
    local.clear();
    opponents.clear();
    external.clear();
    opponentExternal.clear();
    focusStates.delete(local);
    focusStates.delete(external);
    for (const entry of availableForSeat(viewer))
      external.add(externalCard(template, entry));
    selected = -1;
    dragged = null;
    for (let i = 0; i < ownCount; i++) {
      const entry = playerZones[viewer - 1]?.hand?.[i];
      const card = entry
        ? pileCard(template, entry, faceMaterials)
        : copyCard(template, options.fixture === false);
      card.userData.canPlay =
        card.userData.faceDown !== true &&
        (entry?.canPlay ?? options.fixture !== false);
      if (entry && entry.faceUp) {
        card.userData.studyFace = entry.key;
        card.userData.studyLand = entry.key === "land";
      }
      if (entry?.sourceId && entry.faceUp)
        card.userData.sourceId = entry.sourceId;
      if (!entry && options.fixture !== false)
        card.userData.studyFace = i % 3 === 1 ? "land" : "lightning-bolt";
      if (!entry && options.fixture !== false && i % 3 === 1) {
        card.userData.studyLand = true;
        card.traverse((part) => {
          if (part instanceof THREE.Mesh && part.userData.printedFront === true)
            part.material = landMaterial;
        });
      }
      local.add(card);
    }
    opponents.rotation.copy(boards.rotation);
    opponentExternal.rotation.copy(boards.rotation);
    for (const [seat, board] of boards.children.entries()) {
      if (seat === viewer - 1) continue;
      const hand = new THREE.Group();
      hand.userData.seat = seat + 1;
      hand.position.set(
        board.position.x,
        surface + 0.6,
        board.position.z + Math.cos(board.rotation.y) * 9.2,
      );
      hand.rotation.y = board.rotation.y;
      const publicCount = publicHandCounts?.[seat] ?? opponentCount;
      for (let i = 0; i < publicCount; i++) {
        const entry = playerZones[seat]?.hand?.[i];
        const card = entry?.faceUp
          ? pileCard(template, entry, faceMaterials)
          : copyCard(template, true);
        card.userData.isHandCard = false;
        if (entry?.faceUp && !card.userData.faceDown)
          card.userData.studyFace = entry.key;
        const offset = i - (publicCount - 1) / 2;
        card.scale.setScalar(0.74);
        card.position.set(
          offset * Math.min(0.68, 9 / Math.max(1, publicCount)),
          i * 0.022,
          Math.abs(offset) * 0.09,
        );
        card.rotation.set(
          card.userData.faceDown ? Math.PI / 2 - 0.22 : -Math.PI / 2 + 0.22,
          0,
          card.userData.faceDown ? Math.PI + offset * 0.055 : offset * 0.055,
        );
        hand.add(card);
      }
      opponents.add(hand);
      for (const [index, entry] of availableForSeat(seat + 1).entries()) {
        const extra = externalCard(template, entry);
        extra.userData.seat = seat + 1;
        extra.position
          .copy(hand.position)
          .add(
            new THREE.Vector3(5.8 + index * 0.6, 0, 0).applyAxisAngle(
              new THREE.Vector3(0, 1, 0),
              board.rotation.y,
            ),
          );
        extra.scale.setScalar(0.74);
        extra.rotation.set(
          -Math.PI / 2,
          0,
          boards.rotation.y * -1 - board.rotation.y,
        );
        opponentExternal.add(extra);
      }
    }
  }
  const positioned = new WeakSet<THREE.Object3D>();
  const targetPosition = new THREE.Vector3();
  const activeHand = (): THREE.Group => (viewedSeat > 0 ? viewedHand : local);
  const activeExternal = (): THREE.Group =>
    viewedSeat > 0 ? viewedExternal : external;
  const cardAt = (index: number): THREE.Object3D | undefined =>
    index < activeHand().children.length
      ? activeHand().children[index]
      : activeExternal().children[index - activeHand().children.length];
  let externalPoses: (DisplayPose | undefined)[] = [];
  function pose(
    hand: THREE.Group,
    items: readonly (DisplayPose | undefined)[],
    handScale: number,
    blend: number,
    selectedIndex: number,
    depthCount = hand.children.length,
  ): boolean {
    let moving = false;
    // Keep physical sleeves from intersecting; dense fans compress thickness equally.
    const thickness = fanThickness(depthCount);
    for (const [i, card] of hand.children.entries()) {
      if (card === dragged) continue;
      const item = items[i];
      if (!item) {
        card.visible = false;
        windowHidden.add(card);
        continue;
      }
      const reentering = windowHidden.has(card);
      if (reentering) {
        card.visible = true;
        windowHidden.delete(card);
      }
      const active = i === selectedIndex;
      const hidden = card.userData.faceDown === true;
      targetPosition.set(
        item.x * handScale,
        active ? 0.45 : item.y * handScale,
        fanDepth(item.depth, depthCount, handScale, active),
      );
      const delay =
        hand.children.length > 1 ? (i / (hand.children.length - 1)) * 0.1 : 0;
      const arrival = THREE.MathUtils.clamp(
        (entranceTime - delay) / 0.32,
        0,
        1,
      );
      const remaining = (1 - arrival) ** 3;
      targetPosition.y -= remaining * 3.8 * handScale;
      const angle = active ? 0 : item.angle * (hidden ? -1 : 1);
      const amount =
        positioned.has(card) && !reentering && entranceTime >= 0.42 ? blend : 1;
      card.position.lerp(targetPosition, amount);
      card.rotation.set(
        0,
        hidden ? Math.PI : 0,
        THREE.MathUtils.lerp(card.rotation.z, angle, amount),
      );
      card.scale.setScalar(
        THREE.MathUtils.lerp(card.scale.x, handScale, amount),
      );
      card.scale.z *= thickness;
      positioned.add(card);
      moving ||=
        card.position.distanceTo(targetPosition) +
          Math.abs(card.rotation.z - angle) +
          Math.abs(card.scale.x - handScale) >
        0.0001;
    }
    return moving;
  }
  function cameraFan(hand: THREE.Group, externalArea: THREE.Group) {
    const windowIndices = (
      group: THREE.Group,
      key: string,
      indices: number[],
    ): number[] => {
      const state = focusFor(group, key);
      state.configure(indices.length);
      const window =
        indices.length > 12
          ? fanWindow(indices.length, state.position)
          : { start: 0, count: indices.length };
      return indices.slice(window.start, window.start + window.count);
    };
    const handIndices = indicesFor(hand, "hand");
    const handFocus = focusFor(hand);
    handFocus.configure(handIndices.length);
    const dense = handIndices.length > 12;
    const folded = dense
      ? foldedFan(handIndices.length, handFocus.position)
      : [];
    const footprintCount = dense ? 9 : handIndices.length;
    const keys = [
      ...new Set(
        externalArea.children.map((card) => String(card.userData.sourceZone)),
      ),
    ];
    const extraIndices = keys.flatMap((key) =>
      windowIndices(externalArea, key, indicesFor(externalArea, key)),
    );
    const arc = groupedHandFan(
      footprintCount,
      extraIndices.map((index) =>
        String(externalArea.children[index]?.userData.sourceZone),
      ),
      Math.min(8, Math.max(0, footprintCount - 1) * 1.18),
      dense ? folded.map((pose) => ((pose.x + 4.15) / 8.3) * 8) : undefined,
    );
    const halfHeight = Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * 9;
    const fanScale = Math.min(
      0.62,
      (halfHeight * 2 * camera.aspect * 0.92) / arc.width,
    );
    hand.position.set(0, -halfHeight + 1.95 * fanScale - 0.16, -9);
    externalArea.position.copy(hand.position);
    externalArea.scale.setScalar(1);
    const handPoses: (DisplayPose | undefined)[] = Array.from({
      length: hand.children.length,
    });
    const extraPoses: (DisplayPose | undefined)[] = Array.from({
      length: externalArea.children.length,
    });
    handIndices.forEach((index, slot) => {
      const item = arc.hand[slot];
      if (item)
        handPoses[index] = {
          ...item,
          depth: dense ? (folded[slot]?.depth ?? slot) : slot,
          folded: dense && folded[slot]?.folded === true,
        };
    });
    extraIndices.forEach((index, slot) => {
      const item = arc.external[slot];
      if (item)
        extraPoses[index] = { ...item, depth: handIndices.length + slot };
    });
    return {
      scale: fanScale,
      poses: handPoses,
      externalPoses: extraPoses,
      count: handIndices.length + extraIndices.length,
      handIndices,
    };
  }
  /** Both owned and viewed hands share one presentation/gesture mechanism. */
  function frame(
    enabled: boolean,
    focus: number,
    elapsed = 1,
    reduced = false,
  ): boolean {
    local.visible =
      opponents.visible =
      external.visible =
      opponentExternal.visible =
        enabled;
    if (!enabled) {
      wasEnabled = false;
      if (control) control.hidden = true;
      if (panTarget) focusFor(panTarget.group, panTarget.key).end(true);
      panTarget = null;
      return false;
    }
    if (!wasEnabled) entranceTime = 0;
    wasEnabled = true;
    entranceTime = reduced ? 1 : Math.min(1, entranceTime + elapsed);
    let folding = false;
    if (panTarget) {
      const state = focusFor(panTarget.group, panTarget.key);
      folding = state.tick(elapsed, reduced);
      if (folding)
        selected =
          panTarget.offset +
          (panTarget.indices[Math.round(state.position)] ?? 0);
    }
    const own = cameraFan(local, external),
      other = cameraFan(viewedHand, viewedExternal);
    const current = viewedSeat > 0 ? other : own;
    externalPoses = current.externalPoses;
    scale = current.scale;
    poses = current.poses;
    if (control) {
      control.hidden = browsingBlocked || activeHand().children.length <= 12;
      const focus = focusFor(activeHand());
      control.max = String(Math.max(0, activeHand().children.length - 1));
      control.value = String(Math.round(focus.position));
      control.setAttribute(
        "aria-label",
        viewedSeat > 0 ? "Browse viewed hand" : "Browse your hand",
      );
      control.setAttribute(
        "aria-valuetext",
        `Card ${Math.round(focus.position) + 1} of ${activeHand().children.length}`,
      );
      const first = current.poses[current.handIndices[0] ?? 0],
        last = current.poses[current.handIndices.at(-1) ?? 0];
      const halfHeight = Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * 9;
      const center = (((first?.x ?? 0) + (last?.x ?? 0)) / 2) * current.scale;
      control.style.left = `${50 + (center / (halfHeight * 2 * camera.aspect)) * 100}%`;
    }
    for (const hand of [...opponents.children, ...opponentExternal.children])
      hand.visible = focus === 0 || focus === hand.userData.seat;
    const blend = reduced ? 1 : 1 - Math.exp(-elapsed * 22);
    const ownMoving = pose(
      local,
      own.poses,
      own.scale,
      blend,
      viewedSeat > 0 ? -1 : selected,
      own.count,
    );
    const otherMoving = pose(
      viewedHand,
      other.poses,
      other.scale,
      blend,
      viewedSeat > 0 ? selected : -1,
      other.count,
    );
    const extraMoving = pose(
      external,
      own.externalPoses,
      own.scale,
      blend,
      viewedSeat === 0 ? selected - local.children.length : -1,
      own.count,
    );
    const viewedExtraMoving = pose(
      viewedExternal,
      other.externalPoses,
      other.scale,
      blend,
      viewedSeat > 0 ? selected - viewedHand.children.length : -1,
      other.count,
    );
    return (
      ownMoving ||
      otherMoving ||
      extraMoving ||
      viewedExtraMoving ||
      folding ||
      entranceTime < 0.42
    );
  }
  function hover(index: number): boolean {
    if (dragged) return false;
    if (selected === index) return false;
    selected = index;
    const item =
      index < activeHand().children.length
        ? poses[index]
        : externalPoses[index - activeHand().children.length];
    if (index >= 0 && (cardAt(index)?.visible === false || item?.folded)) {
      const target = groupForIndex(index);
      if (target) {
        const state = focusFor(target.group, target.key);
        state.configure(target.indices.length);
        state.reset(target.position);
      }
    }
    return true;
  }
  /** Stable fan slots prevent an enlarged hovered card from trapping the pointer. */
  function pick(raycaster: THREE.Raycaster): number {
    if (!activeHand().visible) return -1;
    const ray = raycaster.ray.clone().applyMatrix4(camera.matrixWorldInverse);
    const distance = (-9 - ray.origin.z) / ray.direction.z;
    if (distance < 0) return -1;
    const point = ray.at(distance, new THREE.Vector3());
    const pickGroup = (
      group: THREE.Group,
      items: readonly (DisplayPose | undefined)[],
    ): number => {
      const visible = items.flatMap((item, index) =>
        item && !item.folded ? [{ item, index }] : [],
      );
      if (!visible.length) return -1;
      const x = (point.x - group.position.x) / scale;
      const slot = fanIndexAt(
        x,
        visible.map((entry) => entry.item),
        1.25,
      );
      const entry = visible[slot];
      const item = entry?.item;
      const index = entry?.index ?? -1;
      if (
        !item ||
        Math.abs(x - item.x) > 1.25 ||
        point.y < group.position.y + (item.y - 1.9) * scale ||
        point.y > group.position.y + (item.y + 2.1) * scale
      )
        return -1;
      return index;
    };
    const extra = pickGroup(activeExternal(), externalPoses);
    const normal =
      extra >= 0
        ? activeHand().children.length + extra
        : pickGroup(activeHand(), poses);
    if (normal >= 0) return normal;
    const raised = cardAt(selected);
    if (raised?.visible) {
      raised.updateWorldMatrix(true, true);
      if (
        raycaster
          .intersectObject(raised, true)
          .some((hit) => hit.object.userData.commanderAura !== true)
      )
        return selected;
    }
    return -1;
  }

  return {
    dispose(): void {
      control?.remove();
      commanderAuraGeometry.dispose();
      commanderAuraMaterial.dispose();
    },
    local,
    get active(): THREE.Group {
      return activeHand();
    },
    get canPlayDragged(): boolean {
      return dragged !== null && dragged.userData.canPlay === true;
    },
    viewedHand,
    viewedExternal,
    viewOpponent,
    opponents,
    external,
    opponentExternal,
    rebuild,
    frame,
    cardAt,
    setBrowsingBlocked(blocked: boolean): void {
      browsingBlocked = blocked;
      if (control)
        control.hidden =
          !wasEnabled || blocked || activeHand().children.length <= 12;
    },
    get currentIndex(): number {
      return Math.max(0, selected);
    },
    canBrowse(index: number): boolean {
      return (groupForIndex(index)?.indices.length ?? 0) > 12;
    },
    hover,
    pick,
    hit(raycaster: THREE.Raycaster): number {
      if (!activeHand().visible) return -1;
      // Grabbing follows the raised face; hover still uses stable resting strips.
      const raised = cardAt(selected);
      if (raised?.visible) {
        raised.updateWorldMatrix(true, true);
        if (
          raycaster
            .intersectObject(raised, true)
            .some((hit) => hit.object.userData.commanderAura !== true)
        )
          return selected;
      }
      return pick(raycaster);
    },
    beginDrag(index: number): boolean {
      if (panTarget) focusFor(panTarget.group, panTarget.key).end(true);
      panTarget = null;
      dragged = cardAt(index) ?? null;
      if (dragged) selected = index;
      return dragged !== null;
    },
    dragTo(raycaster: THREE.Raycaster): void {
      if (!dragged) return;
      const ray = raycaster.ray.clone().applyMatrix4(camera.matrixWorldInverse);
      const distance = (-8.5 - ray.origin.z) / ray.direction.z;
      const point = ray.at(distance, new THREE.Vector3());
      dragged.position.set(
        point.x - (dragged.parent?.position.x ?? 0),
        point.y - (dragged.parent?.position.y ?? 0),
        0.5,
      );
      dragged.rotation.set(0, dragged.userData.faceDown ? Math.PI : 0, 0);
      dragged.scale.setScalar(scale);
    },
    endDrag(slot: number | null): THREE.Object3D | null {
      const card = dragged;
      dragged = null;
      selected = -1;
      if (card && slot !== null && card.parent === activeHand()) {
        const from = activeHand().children.indexOf(card);
        activeHand().children.splice(from, 1);
        activeHand().children.splice(
          THREE.MathUtils.clamp(slot, 0, activeHand().children.length),
          0,
          card,
        );
        selected = activeHand().children.indexOf(card);
      }
      return card;
    },
    slotAt(raycaster: THREE.Raycaster): number {
      const ray = raycaster.ray.clone().applyMatrix4(camera.matrixWorldInverse);
      const point = ray.at(
        (-9 - ray.origin.z) / ray.direction.z,
        new THREE.Vector3(),
      );
      const visible = poses.flatMap((item, index) =>
        item && !item.folded ? [{ item, index }] : [],
      );
      const x = (point.x - activeHand().position.x) / scale;
      // Drops target the nearest slot center, rather than a neighbor's exposed strip.
      return visible.reduce(
        (nearest, entry) =>
          Math.abs(entry.item.x - x) < Math.abs(nearest.item.x - x)
            ? entry
            : nearest,
        visible[0] ?? { item: { x: 0, y: 0, angle: 0, depth: 0 }, index: 0 },
      ).index;
    },
    browse(direction: number): void {
      hover(
        THREE.MathUtils.clamp(
          selected < 0 ? 0 : selected + direction,
          0,
          activeHand().children.length + activeExternal().children.length - 1,
        ),
      );
      const target = groupForIndex(selected);
      if (target) {
        const state = focusFor(target.group, target.key);
        state.configure(target.indices.length);
        state.reset(target.position);
      }
    },
    beginPan(index = Math.max(0, selected)): void {
      panTarget = groupForIndex(index);
      dragged = null;
      if (panTarget) focusFor(panTarget.group, panTarget.key).begin();
    },
    pan(pixels: number, width: number, seconds: number): void {
      if (!panTarget) return;
      const state = focusFor(panTarget.group, panTarget.key);
      state.configure(panTarget.indices.length);
      state.pan(
        (pixels * Math.max(8, panTarget.indices.length * 0.5)) / width,
        seconds,
      );
      selected =
        panTarget.offset + (panTarget.indices[Math.round(state.position)] ?? 0);
    },
    endPan(paused: boolean): void {
      if (panTarget) focusFor(panTarget.group, panTarget.key).end(paused);
    },
    get currentCard(): THREE.Object3D | undefined {
      return cardAt(Math.max(0, selected));
    },
  };
}
