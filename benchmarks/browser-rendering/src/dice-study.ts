import * as THREE from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";
import { diceSets, type DiceSet } from "./dice/catalog";
import { createDie, type DieModel } from "./dice/models";
import { faceUp } from "./dice/faces";
import {
  liftedPosition,
  releaseVelocity,
  type DragSample,
} from "./dice/gesture";
import type { ThemeLayer, ThemePose } from "./dice/theme-layer";
import {
  accountThemesSchema,
  themeDieId,
  type AccountTheme,
} from "./dice/theme-catalog";
import { createRollController } from "./dice/roll-controller";
import {
  DIE_TYPES,
  ROLL_TYPES,
  faceLabel,
  type DieType,
  type ResolvedRoll,
} from "./dice/contracts";
import { restingHeight } from "./dice/solid";
import { fitThrowCamera, type CameraPose } from "./dice/framing";
import {
  createTableThrow,
  sampleTable,
  type BodyState,
  type TableTrack,
  type Launch,
} from "./dice/table-physics";

function required<T extends HTMLElement>(
  id: string,
  constructor: { new (): T },
): T {
  const element = document.getElementById(id);
  if (!(element instanceof constructor))
    throw new Error(`Missing dice study control: ${id}`);
  return element;
}
const canvas = required("table", HTMLCanvasElement),
  stage = required("stage", HTMLDivElement);
const status = required("status", HTMLOutputElement),
  result = required("result", HTMLOutputElement);
const setName = required("set-name", HTMLHeadingElement),
  access = required("set-access", HTMLParagraphElement);

const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
const renderer = new THREE.WebGLRenderer({
  canvas,
  antialias: true,
  alpha: false,
});
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFShadowMap;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.15;
const scene = new THREE.Scene();
scene.background = new THREE.Color("#202322");
const camera = new THREE.PerspectiveCamera(34, 1, 0.5, 80);
const pmrem = new THREE.PMREMGenerator(renderer),
  room = new RoomEnvironment();
const environment = pmrem.fromScene(room, 0.04);
scene.environment = environment.texture;
scene.environmentIntensity = 0.4;
room.dispose();
pmrem.dispose();
scene.add(new THREE.HemisphereLight(0xdde9e3, 0x25302a, 2));
const key = new THREE.DirectionalLight(0xffe7c2, 2.7);
key.position.set(-4, 9, 5);
key.castShadow = true;
key.shadow.mapSize.set(2048, 2048);
Object.assign(key.shadow.camera, {
  left: -8,
  right: 8,
  top: 8,
  bottom: -8,
  near: 0.5,
  far: 30,
});
key.shadow.normalBias = 0.025;
key.shadow.bias = -0.0001;
scene.add(key);
const rim = new THREE.DirectionalLight(0xb5cedf, 1.8);
rim.position.set(6, 4, -5);
scene.add(rim);

const tray = new THREE.Mesh(
  new RoundedBoxGeometry(11.8, 0.32, 7.8, 5, 0.18),
  new THREE.MeshStandardMaterial({
    color: 0x30251f,
    roughness: 0.52,
    metalness: 0.1,
  }),
);
tray.position.y = -0.21;
tray.receiveShadow = true;
scene.add(tray);
const feltCanvas = document.createElement("canvas");
feltCanvas.width = 256;
feltCanvas.height = 256;
const feltContext = feltCanvas.getContext("2d");
if (!feltContext) throw new Error("Felt texture canvas unavailable");
const pixels = feltContext.createImageData(256, 256);
let textureSeed = 73;
for (let i = 0; i < pixels.data.length; i += 4) {
  textureSeed = (Math.imul(textureSeed, 1664525) + 1013904223) >>> 0;
  const grain = ((textureSeed >>> 24) / 255) * 14;
  pixels.data[i] = 30 + grain;
  pixels.data[i + 1] = 51 + grain;
  pixels.data[i + 2] = 44 + grain;
  pixels.data[i + 3] = 255;
}
feltContext.putImageData(pixels, 0, 0);
const felt = new THREE.CanvasTexture(feltCanvas);
felt.colorSpace = THREE.SRGBColorSpace;
felt.wrapS = felt.wrapT = THREE.RepeatWrapping;
felt.repeat.set(5, 4);
const mat = new THREE.Mesh(
  new RoundedBoxGeometry(11.15, 0.08, 7.15, 4, 0.08),
  new THREE.MeshStandardMaterial({ map: felt, roughness: 0.98 }),
);
mat.position.y = -0.045;
mat.receiveShadow = true;
scene.add(mat);

type Die = {
  sides: DieType;
  model: DieModel;
  value: number;
  controller: ReturnType<typeof createRollController>;
  pending: boolean;
  unpresented: ResolvedRoll | null;
  lastEvent: ResolvedRoll | null;
  detached: boolean;
  attempt: number;
  unresolved: boolean;
  physical: BodyState;
};
const themeSelect = required("account-theme", HTMLSelectElement);
const themeCanvas = document.createElement("canvas");
themeCanvas.className = "theme-layer";
themeCanvas.setAttribute("aria-hidden", "true");
canvas.after(themeCanvas);
themeCanvas.hidden = true;
let themeLayer: ThemeLayer | null = null;
let themeActive = false;
let activeTypes: readonly DieType[] = ROLL_TYPES;
let accountThemes: AccountTheme[] = [];
function themePose(die: Die): ThemePose {
  return {
    sides: die.sides,
    position: die.model.group.position,
    rotation: die.model.group.quaternion,
    value: die.value,
  };
}
function proxyMaterials(enabled: boolean): void {
  for (const die of dice)
    die.model.group.traverse((object) => {
      if (object instanceof THREE.Mesh) {
        const materials: THREE.Material[] = Array.isArray(object.material)
          ? object.material
          : [object.material];
        for (const material of materials) {
          material.colorWrite = !enabled || die.sides === 2;
          material.depthWrite = !enabled || die.sides === 2;
        }
      }
    });
}
const cameraTarget = new THREE.Vector3();
let dice: Die[] = [],
  selectedSet: DiceSet = diceSets[0];
let pending = false,
  disposed = false;
let playback: {
  track: TableTrack;
  began: number;
  events: ReadonlyMap<DieType, string>;
} | null = null;
const results = new Map<DieType, number>();
function busy(die: Die): boolean {
  const simulated =
    playback?.track.frames[0]?.bodies.some((b) => b.sides === die.sides) ??
    false;
  return (
    die.pending ||
    (simulated && (die.unpresented !== null || !die.physical.sleeping))
  );
}
function anyRoll(): boolean {
  return playback !== null || dice.some((d) => d.pending);
}
let cameraEnd: CameraPose | null = null;
function frameRolls(): void {
  cameraEnd = fitThrowCamera(
    playback
      ? playback.track.frames.flatMap((f) => f.bodies.map((b) => b.position))
      : [],
    dice
      .filter((d) => activeTypes.includes(d.sides))
      .map((d) => d.model.group.position),
    camera.aspect,
    camera.fov,
  );
}
/** Publish a fully simulated branch; face mappings stay fixed for its entire lifetime. */
function planTable(launch?: Launch, wakeSleeping = false): boolean {
  const included = dice.filter(
    (d) => activeTypes.includes(d.sides) && drag?.die !== d && !d.detached,
  );
  const accepted = new Map<DieType, number>();
  for (const die of included) {
    const value = die.unpresented?.value ?? results.get(die.sides);
    if (value !== undefined) accepted.set(die.sides, value);
  }
  const input = {
    bodies: included.map((d) =>
      wakeSleeping ? { ...d.physical, sleeping: false } : d.physical,
    ),
    results: accepted,
    ...(themeActive && themeLayer
      ? {
          readingNormals: new Map(
            [...accepted].map(([sides, value]) => [
              sides,
              sides === 2
                ? new THREE.Vector3(0, value === 1 ? 1 : -1, 0)
                : themeLayer!.readingNormal(sides, value),
            ]),
          ),
        }
      : {}),
    ...(launch ? { launch } : {}),
  };
  const outcome = createTableThrow(input);
  if (outcome.status === "error") {
    status.value = outcome.message;
    return false;
  }
  playback = {
    track: outcome.track,
    began: performance.now(),
    events: new Map(
      included.flatMap((d) =>
        d.lastEvent ? [[d.sides, d.lastEvent.eventId] as const] : [],
      ),
    ),
  };
  frameRolls();
  updateControls();
  wake();
  return true;
}
let frame = 0;
function draw(): void {
  if (!disposed) {
    renderer.render(scene, camera);
    if (themeActive)
      themeLayer?.render(
        dice.map(themePose),
        camera,
        performance.now(),
        reducedMotion.matches,
      );
  }
}
function updateControls(): void {
  themeSelect.disabled =
    pending || Boolean(drag) || anyRoll() || dice.some((d) => d.unresolved);
  document
    .querySelectorAll<HTMLButtonElement>("button[data-set]")
    .forEach(
      (button) =>
        (button.disabled =
          pending ||
          Boolean(drag) ||
          anyRoll() ||
          dice.some((d) => d.unresolved)),
    );
  document
    .querySelectorAll<HTMLButtonElement>("button[data-roll]")
    .forEach((button) => {
      const die = dice.find((d) => String(d.sides) === button.dataset["roll"]);
      button.disabled =
        pending ||
        !die ||
        busy(die) ||
        drag?.die === die ||
        !activeTypes.includes(die.sides);
      button.title =
        die && !activeTypes.includes(die.sides)
          ? "Not included in this source theme"
          : "";
    });
  canvas.style.cursor = drag ? "grabbing" : "grab";
}
let previousFrame = 0;
function tick(now: number): void {
  frame = 0;
  if (document.hidden || disposed) return;
  if (
    !playback &&
    !drag &&
    !themeActive &&
    !reducedMotion.matches &&
    dice.some((d) => d.model.animate) &&
    now - previousFrame < 1000 / 30
  ) {
    frame = requestAnimationFrame(tick);
    return;
  }
  const dt = Math.min(
    previousFrame ? (now - previousFrame) / 1000 : 1 / 60,
    0.05,
  );
  previousFrame = now;
  if (cameraEnd && !drag) {
    camera.position.lerp(cameraEnd.position, 0.12);
    cameraTarget.lerp(cameraEnd.target, 0.12);
    camera.lookAt(cameraTarget);
  }
  if (playback) {
    const elapsed = reducedMotion.matches
      ? playback.track.duration
      : (now - playback.began) / 1000;
    for (const state of sampleTable(playback.track, elapsed)) {
      const die = dice.find((d) => d.sides === state.sides);
      if (!die || drag?.die === die) continue;
      die.physical = state;
      die.model.group.position.copy(state.position);
      die.model.group.quaternion.copy(state.rotation);
    }
    if (elapsed >= playback.track.duration) {
      for (const [sides, value] of playback.track.results) {
        const die = dice.find((d) => d.sides === sides);
        if (!die || die.lastEvent?.eventId !== playback.events.get(sides))
          continue;
        if (playback.track.cocked.has(sides)) {
          die.unpresented = die.lastEvent;
          die.unresolved = true;
          continue;
        }
        if (
          !die.unpresented ||
          die.unpresented.eventId !== playback.events.get(sides)
        )
          continue;
        die.value = value;
        die.unpresented = null;
        die.unresolved = false;
        results.set(sides, value);
      }
      result.value =
        [...results]
          .map(
            ([type, value]) =>
              `${type === 2 ? "Coin" : `d${type}`}: ${faceLabel(type, value)}`,
          )
          .join(" · ") || "Ready to roll";
      status.value = playback.track.cocked.size
        ? "A die is cocked. Roll it again to present the same result."
        : "Settled on the shared table.";
      playback = null;
      updateControls();
    }
  }
  if (drag) {
    const die = drag.die,
      old = die.physical.position.clone();
    die.physical.position.lerp(drag.target, 1 - Math.exp(-18 * dt));
    die.physical.velocity
      .copy(die.physical.position)
      .sub(old)
      .divideScalar(Math.max(dt, 0.001));
    die.model.group.position.copy(die.physical.position);
  }
  if (!themeActive)
    for (const die of dice)
      die.model.animate?.(reducedMotion.matches ? 0 : now / 1000);
  updateControls();
  draw();
  if (
    (playback ||
      drag ||
      (!reducedMotion.matches &&
        (themeActive || dice.some((d) => d.model.animate)))) &&
    !document.hidden
  )
    frame = requestAnimationFrame(tick);
}
function wake(): void {
  if (!frame && !document.hidden) frame = requestAnimationFrame(tick);
}
function equip(set: DiceSet): void {
  themeActive = false;
  activeTypes = ROLL_TYPES;
  themeCanvas.hidden = true;
  themeSelect.value = "";
  themeLayer?.clear();
  for (const die of dice) {
    scene.remove(die.model.group);
    die.model.dispose();
    die.controller.dispose();
  }
  selectedSet = set;
  results.clear();
  playback = null;
  dice = ROLL_TYPES.map((sides, i) => {
    const model = createDie(sides, set),
      value = sides === 100 ? 90 : sides === 6 || sides === 2 ? 1 : sides;
    const row = i < 4,
      x = row ? -3.3 + i * 2.2 : -3.3 + (i - 4) * 2.2;
    model.group.position.set(
      x,
      restingHeight(sides, value),
      row ? -1.25 : 1.15,
    );
    const normal = model.normals.get(value);
    if (normal) model.group.quaternion.copy(faceUp(normal, 0.15));
    scene.add(model.group);
    return {
      sides,
      model,
      value,
      controller: createRollController(),
      pending: false,
      unpresented: null,
      lastEvent: null,
      detached: false,
      attempt: 0,
      unresolved: false,
      physical: {
        sides,
        position: model.group.position.clone(),
        rotation: model.group.quaternion.clone(),
        velocity: new THREE.Vector3(),
        angularVelocity: new THREE.Vector3(),
        sleeping: true,
      },
    };
  });
  document
    .querySelectorAll<HTMLButtonElement>("button[data-set]")
    .forEach((button) =>
      button.setAttribute(
        "aria-pressed",
        String(button.dataset["set"] === set.id),
      ),
    );
  setName.textContent = set.name;
  access.textContent = set.access;
  result.value = "Ready to roll";
  status.value = "Pick up and release to roll.";
  resize();
  wake();
}

/** A release requests a result; new throws branch from the latest displayed world state when it arrives. */
async function roll(die: Die, velocity: THREE.Vector3): Promise<void> {
  if (pending || busy(die) || disposed) return;
  die.pending = true;
  die.unresolved = true;
  updateControls();
  status.value = "Requesting roll…";
  const outcome = die.unpresented
    ? { status: "success" as const, event: die.unpresented }
    : await die.controller.roll(die.sides);
  if (disposed) return;
  die.pending = false;
  const wasDetached = die.detached;
  die.detached = false;
  if (outcome.status === "error") {
    if (wasDetached)
      planTable({
        sides: die.sides,
        velocity: new THREE.Vector3(),
        seed: 0,
        spin: false,
      });
    status.value = outcome.message;
    updateControls();
    wake();
    return;
  }
  die.unpresented = outcome.event;
  die.lastEvent = outcome.event;
  const launch = {
    sides: die.sides,
    velocity,
    seed: (outcome.event.visualSeed + Math.imul(die.attempt, 0x9e3779b9)) >>> 0,
  };
  if (!planTable(launch)) {
    die.attempt++;
    updateControls();
    wake();
    return;
  }
  die.attempt = 0;
  result.value = "Rolling…";
  status.value = "Dice are rolling together.";
}

const raycaster = new THREE.Raycaster(),
  pointer = new THREE.Vector2(),
  plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), -0.9);
function tablePoint(event: PointerEvent): THREE.Vector3 | null {
  const rect = canvas.getBoundingClientRect();
  pointer.set(
    ((event.clientX - rect.left) / rect.width) * 2 - 1,
    -((event.clientY - rect.top) / rect.height) * 2 + 1,
  );
  raycaster.setFromCamera(pointer, camera);
  return raycaster.ray.intersectPlane(plane, new THREE.Vector3());
}
type Drag = {
  id: number;
  die: Die;
  offset: THREE.Vector3;
  target: THREE.Vector3;
  samples: DragSample[];
};
let drag: Drag | null = null;
canvas.addEventListener("pointerdown", (event) => {
  if (event.button !== 0 || pending || drag) return;
  const point = tablePoint(event);
  if (!point) return;
  const hits = raycaster.intersectObjects(
    dice
      .filter((die) => activeTypes.includes(die.sides))
      .map((die) => die.model.group),
    true,
  );
  const hit = hits[0];
  if (!hit) return;
  const die = dice.find((candidate) => {
    let object: THREE.Object3D | null = hit.object;
    while (object) {
      if (object === candidate.model.group) return true;
      object = object.parent;
    }
    return false;
  });
  if (!die || busy(die) || !activeTypes.includes(die.sides)) return;
  // A camera-facing plane permits vertical movement rather than projecting every gesture onto the felt.
  plane.setFromNormalAndCoplanarPoint(
    camera.getWorldDirection(new THREE.Vector3()),
    hit.point,
  );
  const grabPoint = tablePoint(event);
  if (!grabPoint) return;
  const offset = die.model.group.position.clone().sub(grabPoint);
  const previous = die.physical;
  die.physical = {
    ...previous,
    position: die.model.group.position.clone(),
    rotation: die.model.group.quaternion.clone(),
    velocity: new THREE.Vector3(),
    angularVelocity: new THREE.Vector3(),
    sleeping: false,
  };
  drag = {
    id: event.pointerId,
    die,
    offset,
    target: liftedPosition(grabPoint, offset, 1.6),
    samples: [
      {
        point: liftedPosition(grabPoint, offset, 1.6),
        time: performance.now(),
      },
    ],
  };
  if (!planTable(undefined, true)) {
    drag = null;
    die.physical = previous;
    return;
  }
  die.detached = true;
  canvas.setPointerCapture(event.pointerId);
  canvas.style.cursor = "grabbing";
  status.value = "Lift, then release to drop or throw.";
  wake();
});
canvas.addEventListener("pointermove", (event) => {
  if (!drag || event.pointerId !== drag.id) return;
  const point = tablePoint(event);
  if (!point) return;
  drag.target.copy(liftedPosition(point, drag.offset, 1.6));
  const now = performance.now();
  drag.samples.push({ point: drag.target.clone(), time: now });
  drag.samples = drag.samples.filter((sample) => now - sample.time <= 120);
  wake();
});
function endDrag(event: PointerEvent, cancel: boolean): void {
  if (!drag || event.pointerId !== drag.id) return;
  const current = drag;
  drag = null;
  if (canvas.hasPointerCapture(event.pointerId))
    canvas.releasePointerCapture(event.pointerId);
  if (cancel) {
    current.die.detached = false;
    const placed = planTable({
      sides: current.die.sides,
      velocity: new THREE.Vector3(),
      seed: 0,
      spin: false,
    });
    if (!placed) status.value = "Drop interrupted. Release again to roll.";
    updateControls();
    wake();
    return;
  }
  const velocity = releaseVelocity(
    current.samples,
    current.target,
    performance.now(),
  );
  void roll(current.die, velocity);
}
canvas.addEventListener("pointerup", (event) => endDrag(event, false));
canvas.addEventListener("pointercancel", (event) => endDrag(event, true));
canvas.addEventListener("lostpointercapture", (event) => endDrag(event, true));
document
  .querySelectorAll<HTMLButtonElement>("button[data-set]")
  .forEach((button) =>
    button.addEventListener("click", () => {
      const set = diceSets.find(
        (candidate) => candidate.id === button.dataset["set"],
      );
      if (
        set &&
        !pending &&
        !anyRoll() &&
        !dice.some((d) => d.unresolved) &&
        !drag
      )
        equip(set);
    }),
  );
document
  .querySelectorAll<HTMLButtonElement>("button[data-roll]")
  .forEach((button) =>
    button.addEventListener("click", () => {
      const die = dice.find(
        (candidate) => String(candidate.sides) === button.dataset["roll"],
      );
      if (die && drag?.die !== die)
        void roll(die, new THREE.Vector3(die.sides === 6 ? 6 : -6, 5, -2.5));
    }),
  );
function resize(): void {
  const width = stage.clientWidth,
    height = stage.clientHeight;
  renderer.setSize(width, height, false);
  camera.aspect = width / height;
  const narrow = camera.aspect < 1;
  camera.position.set(narrow ? 2 : 4, narrow ? 9 : 7, 8);
  camera.fov = narrow ? 40 : 34;
  cameraTarget.set(0, 0, 0);
  const fitted = fitThrowCamera(
    [],
    dice.map((d) => d.model.group.position),
    camera.aspect,
    camera.fov,
  );
  camera.position.copy(fitted.position);
  cameraTarget.copy(fitted.target);
  cameraEnd = null;
  camera.lookAt(cameraTarget);
  const inspect = new URLSearchParams(location.search).get("inspect");
  const inspected = dice.find((die) => String(die.sides) === inspect);
  if (inspected && !anyRoll()) {
    cameraTarget.copy(inspected.model.group.position);
    camera.position.copy(cameraTarget).add(new THREE.Vector3(1.7, 2.1, 3.5));
    camera.lookAt(cameraTarget);
  }
  camera.updateProjectionMatrix();
  themeLayer?.resize(width, height);
  if (playback) frameRolls();
  wake();
}
const observer = new ResizeObserver(resize);
observer.observe(stage);
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    if (drag) {
      const held = drag.die;
      drag = null;
      held.detached = false;
      planTable({
        sides: held.sides,
        velocity: new THREE.Vector3(),
        seed: 0,
        spin: false,
      });
      status.value = "Pickup interrupted; die released under gravity.";
      updateControls();
    }
  } else wake();
});
reducedMotion.addEventListener("change", wake);
window.addEventListener("pagehide", (event: PageTransitionEvent) => {
  if (frame) cancelAnimationFrame(frame);
  frame = 0;
  if (event.persisted) return;
  disposed = true;
  dice.forEach((die) => die.controller.dispose());
  observer.disconnect();
  dice.forEach((die) => die.model.dispose());
  tray.geometry.dispose();
  tray.material.dispose();
  mat.geometry.dispose();
  mat.material.dispose();
  felt.dispose();
  environment.dispose();
  renderer.dispose();
  themeLayer?.dispose();
});
window.addEventListener("pageshow", wake);
equip(selectedSet);
resize();

async function loadAccountThemes(): Promise<void> {
  try {
    const response = await fetch("/dice-themes.json");
    if (!response.ok) throw new Error("Theme catalogue unavailable");
    const raw: unknown = await response.json();
    accountThemes = accountThemesSchema.parse(raw);
    for (const theme of accountThemes) {
      const option = document.createElement("option");
      option.value = theme.id;
      const missing = DIE_TYPES.filter((sides) => !themeDieId(theme, sides));

      option.textContent = missing.length
        ? `${theme.name} (missing ${missing.map((s) => `d${s}`).join(", ")})`
        : theme.name;
      themeSelect.append(option);
    }
  } catch (error) {
    console.error("dddice catalogue failed", error);
    status.value = "Theme catalogue unavailable. Reload to retry.";
  }
}
themeSelect.addEventListener("change", () => {
  if (pending || anyRoll() || dice.some((d) => d.unresolved) || drag) return;
  const theme = accountThemes.find(
    (candidate) => candidate.id === themeSelect.value,
  );
  if (!theme) {
    equip(selectedSet);
    return;
  }
  void (async () => {
    pending = true;
    updateControls();
    status.value = "Loading original dddice theme…";
    themeActive = false;
    proxyMaterials(false);
    themeCanvas.hidden = true;
    try {
      const { createThemeLayer } = await import("./dice/theme-layer");
      if (disposed) return;
      themeCanvas.hidden = false;
      themeCanvas.style.visibility = "hidden";
      themeLayer ??= createThemeLayer(themeCanvas);
      themeLayer.resize(stage.clientWidth, stage.clientHeight);
      await themeLayer.load(theme);
      if (disposed) return;
      dice.forEach((die) => themeLayer?.align(themePose(die)));
      activeTypes = [
        ...DIE_TYPES.filter((type) => themeDieId(theme, type) !== null),
        2,
      ];
      dice.forEach(
        (die) => (die.model.group.visible = activeTypes.includes(die.sides)),
      );
      themeActive = true;
      dice.forEach((die) => {
        die.physical.rotation.copy(die.model.group.quaternion);
        die.physical.position.copy(die.model.group.position);
        die.physical.sleeping = true;
        die.physical.velocity.set(0, 0, 0);
        die.physical.angularVelocity.set(0, 0, 0);
      });
      proxyMaterials(true);
      themeCanvas.hidden = false;
      themeCanvas.style.visibility = "visible";
      setName.textContent = theme.name;
      const missing = DIE_TYPES.filter((type) => !activeTypes.includes(type));
      access.textContent = missing.length
        ? `dddice · No ${missing.map((type) => `d${type}`).join(", ")} in source set`
        : "dddice · Spellwake coin";
      document
        .querySelectorAll<HTMLButtonElement>("button[data-set]")
        .forEach((button) => button.setAttribute("aria-pressed", "false"));
      status.value = "Pick up and release to roll.";
    } catch (error) {
      console.error("dddice theme failed", error);
      themeLayer?.clear();
      activeTypes = ROLL_TYPES;
      dice.forEach((die) => (die.model.group.visible = true));
      themeCanvas.hidden = true;
      themeSelect.value = "";
      setName.textContent = selectedSet.name;
      access.textContent = selectedSet.access;
      document
        .querySelectorAll<HTMLButtonElement>("button[data-set]")
        .forEach((button) =>
          button.setAttribute(
            "aria-pressed",
            String(button.dataset["set"] === selectedSet.id),
          ),
        );
      status.value = "Theme could not load. Select again to retry.";
    } finally {
      pending = false;
      updateControls();
      wake();
    }
  })();
});
void loadAccountThemes();
