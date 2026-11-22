import * as THREE from "three";
import {
  CARD,
  PACK,
  PackSheet,
  cardGeometry,
  restShape,
  tearLine,
  thickness,
  type Deform,
  type Side,
} from "./pack-geometry";
import { colour, foilMaterial, light } from "./pack-model";
import { printPack, raySprite, softSprite, type PackPrint } from "./pack-print";
import {
  bezier,
  clamp01,
  mix,
  openingScript,
  smoothstep,
  span,
  type MotionBeats,
  type MotionCurves,
  type OpeningScript,
} from "./pack-timeline";
import {
  CAMERA,
  framePack,
  placeRect,
  worldPerPixel,
  type Rect,
} from "./stage-frame";

export type { Rect } from "./stage-frame";

export type PackStageOptions = {
  print: PackPrint;
  curves: MotionCurves;
  beats: MotionBeats;
  /** The card back the page shows, so dealt cards land looking identical. */
  cardBackUrl: string;
  /** The page rectangle of the wrapper that was opened, for the lift off the shelf. */
  from?: Rect | undefined;
  signal?: AbortSignal;
};

/** A mounted opening stage. */
export type PackStage = {
  /** Frame the waiting pack inside this canvas-relative page area. Ignored once it tears. */
  setArea: (area: Rect) => void;
  /**
   * The cards are known: tear the wrapper, lift the cards and deal one to
   * each slot, in order. Resolves once the last card has landed.
   */
  open: (slots: readonly Rect[]) => Promise<void>;
  /** Land everything now, for a viewer who skips ahead. */
  finish: () => void;
  dispose: () => void;
};

/** A card-sized soft shadow matching the page's `0 12px 26px #000b`. */
function cardShadow(): THREE.CanvasTexture {
  const width = 160;
  const height = Math.round((width * 88) / 63);
  const pad = 56;
  const element = document.createElement("canvas");
  element.width = width + pad * 2;
  element.height = height + pad * 2;
  const context = element.getContext("2d");
  if (!context) throw new Error("A 2D canvas is needed for card shadows.");
  // Draw the card far off-canvas so only its blurred shadow lands inside.
  context.shadowColor = "rgba(0,0,0,0.73)";
  context.shadowBlur = (26 / 184) * width;
  context.shadowOffsetX = 4000;
  context.fillStyle = "#000";
  context.fillRect(pad - 4000, pad, width, height);
  const map = new THREE.CanvasTexture(element);
  map.colorSpace = THREE.SRGBColorSpace;
  return map;
}

/** Flecks of foil thrown off the tear. */
type Fleck = {
  born: number;
  life: number;
  position: THREE.Vector3;
  velocity: THREE.Vector3;
  spin: THREE.Vector3;
  rotation: THREE.Euler;
  size: number;
};
/** Motes of light drifting up out of the open pack. */
type Mote = {
  born: number;
  life: number;
  x: number;
  y: number;
  z: number;
  drift: number;
  rise: number;
  size: number;
};

type Card = {
  mesh: THREE.Group;
  uniform: { value: number };
  shadow: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial>;
  start: {
    position: THREE.Vector3;
    quaternion: THREE.Quaternion;
    scale: THREE.Vector3;
  } | null;
  target: { x: number; y: number; width: number; height: number } | null;
  wobble: number;
};

/**
 * Mount the 3D opening stage over `host`. The wrapper is printed and
 * lifted into place at once; it waits, breathing, until `open` is called.
 */
export async function mountPackStage(
  host: HTMLElement,
  options: PackStageOptions,
): Promise<PackStage> {
  options.signal?.throwIfAborted();
  const coarse = window.matchMedia("(pointer: coarse)").matches;
  const compact = Math.min(window.innerWidth, window.innerHeight) < 600;
  const detail = coarse || compact ? "reduced" : "full";
  const ease = {
    arrive: bezier(options.curves.arrive),
    depart: bezier(options.curves.depart),
    travel: bezier(options.curves.travel),
  };

  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true,
    powerPreference: "high-performance",
  });
  renderer.setPixelRatio(
    // The stage fills the window; past 1.5x the extra pixels cost frames, not clarity.
    Math.min(window.devicePixelRatio, detail === "full" ? 1.5 : 1.25),
  );
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.NeutralToneMapping;
  renderer.toneMappingExposure = 1.05;
  renderer.setClearColor(0x000000, 0);
  const canvas = renderer.domElement;
  canvas.setAttribute("aria-hidden", "true");
  canvas.style.position = "absolute";
  canvas.style.inset = "0";
  canvas.style.width = "100%";
  canvas.style.height = "100%";
  canvas.style.pointerEvents = "none";
  host.append(canvas);

  const scene = new THREE.Scene();
  const environment = light(scene, renderer);
  const camera = new THREE.PerspectiveCamera(CAMERA.fov, 1, 1, 30);
  camera.position.set(0, 0, CAMERA.distance);

  const disposables: { dispose: () => void }[] = [];
  const track = <T extends { dispose: () => void }>(value: T): T => {
    disposables.push(value);
    return value;
  };

  // ─── The wrapper ──────────────────────────────────────────────────────────
  const printed = await printPack(options.print, {
    anisotropy: Math.min(8, renderer.capabilities.getMaxAnisotropy()),
    detail,
    ...(options.signal ? { signal: options.signal } : {}),
  });
  if (options.signal?.aborted) {
    printed.dispose();
    renderer.dispose();
    canvas.remove();
    options.signal.throwIfAborted();
  }
  const palette = {
    hue: colour(options.print.palette.hue).color,
    glow: colour(options.print.palette.glow).color,
  };

  const foil = (side: "front" | "back"): THREE.MeshPhysicalMaterial =>
    track(foilMaterial(printed[side]));
  const frontFoil = foil("front");
  const backFoil = foil("back");

  const root = new THREE.Group();
  const rig = new THREE.Group();
  root.add(rig);
  scene.add(root);
  const sheets = {
    body: [new PackSheet(1, "body"), new PackSheet(-1, "body")] as const,
    strip: [new PackSheet(1, "strip"), new PackSheet(-1, "strip")] as const,
  };
  const strip = new THREE.Group();
  // The strip pivots about the tear line, so its flight starts where it broke.
  strip.position.y = PACK.tearY;
  rig.add(strip);
  for (const sheet of [...sheets.body, ...sheets.strip]) {
    track(sheet);
    const mesh = new THREE.Mesh(
      sheet.geometry,
      sheet.side === 1 ? frontFoil : backFoil,
    );
    if (sheet.region === "strip") {
      mesh.position.y = -PACK.tearY;
      strip.add(mesh);
    } else rig.add(mesh);
  }

  // Light behind the pack, in the set's glow.
  const haloTexture = track(
    softSprite("rgba(255,255,255,0.9)", "rgba(255,255,255,0)"),
  );
  const halo = new THREE.Mesh(
    track(new THREE.PlaneGeometry(1, 1)),
    track(
      new THREE.MeshBasicMaterial({
        map: haloTexture,
        color: palette.glow.clone().lerp(new THREE.Color(0xffffff), 0.15),
        transparent: true,
        depthWrite: false,
        opacity: 0,
        toneMapped: false,
      }),
    ),
  );
  halo.scale.set(2.6, 3.4, 1);
  halo.position.z = -0.35;
  halo.renderOrder = -1;
  rig.add(halo);

  // The torn edges glow where the magic leaks out.
  const warm = new THREE.Color(0xffe2a8);
  const rimMaterial = track(
    new THREE.MeshBasicMaterial({
      vertexColors: true,
      transparent: true,
      depthWrite: false,
      side: THREE.DoubleSide,
      toneMapped: false,
    }),
  );
  const RIM_COLUMNS = 120;
  type Rim = {
    geometry: THREE.BufferGeometry;
    side: Side;
    region: "body" | "strip";
  };
  const rims: Rim[] = [];
  for (const region of ["body", "strip"] as const) {
    for (const side of [1, -1] as const) {
      const geometry = track(new THREE.BufferGeometry());
      const count = (RIM_COLUMNS + 1) * 2;
      geometry.setAttribute(
        "position",
        new THREE.BufferAttribute(new Float32Array(count * 3), 3),
      );
      geometry.setAttribute(
        "color",
        new THREE.BufferAttribute(new Float32Array(count * 4), 4),
      );
      const indices: number[] = [];
      for (let column = 0; column < RIM_COLUMNS; column += 1) {
        const a = column * 2;
        indices.push(a, a + 2, a + 1, a + 1, a + 2, a + 3);
      }
      geometry.setIndex(indices);
      const mesh = new THREE.Mesh(geometry, rimMaterial);
      mesh.renderOrder = 2;
      mesh.frustumCulled = false;
      if (region === "strip") {
        mesh.position.y = -PACK.tearY;
        strip.add(mesh);
      } else rig.add(mesh);
      rims.push({ geometry, side, region });
    }
  }

  // Light pouring out of the mouth.
  const mouthLight = new THREE.PointLight(0xffd391, 0, 3.2, 1.6);
  mouthLight.position.set(0, PACK.tearY + 0.05, 0.12);
  // Outside the rig, so hiding the wrapper never changes the scene's light
  // count: that would recompile every shader mid-animation.
  root.add(mouthLight);
  const glowTexture = track(
    softSprite("rgba(255,244,214,1)", "rgba(255,214,140,0)"),
  );
  const mouthGlow = new THREE.Mesh(
    track(new THREE.PlaneGeometry(1, 1)),
    track(
      new THREE.MeshBasicMaterial({
        map: glowTexture,
        transparent: true,
        depthWrite: false,
        opacity: 0,
        toneMapped: false,
      }),
    ),
  );
  mouthGlow.position.set(0, PACK.tearY + 0.01, 0.06);
  mouthGlow.scale.set(1.05, 0.16, 1);
  mouthGlow.renderOrder = 3;
  rig.add(mouthGlow);
  const rayTexture = track(raySprite());
  const rayGeometry = track(new THREE.PlaneGeometry(1, 1).translate(0, 0.5, 0));
  const rays: {
    mesh: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshBasicMaterial>;
    base: number;
    phase: number;
  }[] = [];
  const rayGroup = new THREE.Group();
  rayGroup.position.set(0, PACK.tearY - 0.02, 0.02);
  rig.add(rayGroup);
  const rayColor = warm.clone().lerp(palette.glow, 0.35);
  const RAYS = detail === "full" ? 17 : 11;
  for (let index = 0; index < RAYS; index += 1) {
    const t = index / (RAYS - 1);
    const material = track(
      new THREE.MeshBasicMaterial({
        map: rayTexture,
        color: rayColor,
        transparent: true,
        depthWrite: false,
        opacity: 0,
        toneMapped: false,
        side: THREE.DoubleSide,
      }),
    );
    const mesh = new THREE.Mesh(rayGeometry, material);
    const angle = (t - 0.5) * 0.9 + Math.sin(index * 12.9) * 0.08;
    mesh.rotation.z = angle;
    mesh.position.x = (t - 0.5) * 0.7;
    mesh.scale.set(
      0.035 + (index % 4) * 0.02,
      0.45 + Math.abs(Math.sin(index * 4.1)) * 0.55,
      1,
    );
    mesh.renderOrder = 1;
    rayGroup.add(mesh);
    rays.push({
      mesh,
      base: 0.25 + 0.35 * Math.abs(Math.sin(index * 7.7)),
      phase: index * 1.7,
    });
  }

  // Foil flecks.
  const FLECKS = detail === "full" ? 110 : 60;
  const fleckMesh = new THREE.InstancedMesh(
    track(new THREE.PlaneGeometry(0.016, 0.011)),
    track(
      new THREE.MeshStandardMaterial({
        metalness: 0.65,
        roughness: 0.3,
        side: THREE.DoubleSide,
        envMapIntensity: 2.2,
        emissive: 0xffd9a0,
        emissiveIntensity: 0.55,
      }),
    ),
    FLECKS,
  );
  fleckMesh.frustumCulled = false;
  fleckMesh.count = 0;
  const fleckColors = [
    palette.hue.clone().lerp(new THREE.Color(0xffffff), 0.35),
    new THREE.Color(0xf2dba6),
    new THREE.Color(0xd9dde2),
  ];
  for (let index = 0; index < FLECKS; index += 1)
    fleckMesh.setColorAt(
      index,
      fleckColors[index % fleckColors.length] ?? warm,
    );
  root.add(fleckMesh);
  const flecks: Fleck[] = [];

  // Motes of light.
  const MOTES = detail === "full" ? 90 : 50;
  const moteGeometry = track(new THREE.BufferGeometry());
  moteGeometry.setAttribute(
    "position",
    new THREE.BufferAttribute(new Float32Array(MOTES * 3), 3),
  );
  moteGeometry.setAttribute(
    "alpha",
    new THREE.BufferAttribute(new Float32Array(MOTES), 1),
  );
  moteGeometry.setAttribute(
    "size",
    new THREE.BufferAttribute(new Float32Array(MOTES), 1),
  );
  const moteMaterial = track(
    new THREE.ShaderMaterial({
      transparent: true,
      depthWrite: false,
      premultipliedAlpha: true,
      uniforms: {
        uScale: { value: 1 },
        uColor: { value: warm.clone().lerp(palette.glow, 0.25) },
      },
      vertexShader: `
        attribute float alpha;
        attribute float size;
        uniform float uScale;
        varying float vAlpha;
        void main() {
          vAlpha = alpha;
          vec4 view = modelViewMatrix * vec4(position, 1.0);
          gl_PointSize = size * uScale / -view.z;
          gl_Position = projectionMatrix * view;
        }`,
      fragmentShader: `
        uniform vec3 uColor;
        varying float vAlpha;
        void main() {
          float d = length(gl_PointCoord - 0.5) * 2.0;
          float core = smoothstep(1.0, 0.0, d);
          float a = core * core * vAlpha;
          gl_FragColor = vec4(mix(uColor, vec3(1.0), core * 0.6) * a, a);
        }`,
    }),
  );
  const motePoints = new THREE.Points(moteGeometry, moteMaterial);
  motePoints.frustumCulled = false;
  motePoints.renderOrder = 4;
  root.add(motePoints);
  const motes: Mote[] = [];

  // ─── Cards ────────────────────────────────────────────────────────────────
  const cardBack = await new THREE.TextureLoader()
    .loadAsync(options.cardBackUrl)
    .catch(() => null);
  if (cardBack) {
    cardBack.colorSpace = THREE.SRGBColorSpace;
    cardBack.anisotropy = Math.min(8, renderer.capabilities.getMaxAnisotropy());
    track(cardBack);
  }
  const cardShape = track(cardGeometry());
  const cardReverse = track(
    new THREE.MeshStandardMaterial({
      color: 0x1a1612,
      roughness: 0.7,
      side: THREE.FrontSide,
    }),
  );
  const shadowTexture = track(cardShadow());
  const shadowGeometry = track(new THREE.PlaneGeometry(1, 1));
  const stack = new THREE.Group();
  rig.add(stack);
  let cards: Card[] = [];

  const buildCards = (count: number): void => {
    cards = Array.from({ length: count }, (_, index) => {
      const uniform = { value: 0 };
      const material = track(
        new THREE.MeshStandardMaterial({
          ...(cardBack ? { map: cardBack } : { color: 0x2a2219 }),
          roughness: 0.82,
          metalness: 0,
          envMapIntensity: 0.55,
          toneMapped: false,
        }),
      );
      // Lit in flight, then exactly the page's image once it lands.
      material.onBeforeCompile = (shader) => {
        shader.uniforms.uFlat = uniform;
        shader.fragmentShader = shader.fragmentShader
          .replace(
            "#include <common>",
            "#include <common>\nuniform float uFlat;",
          )
          .replace(
            "#include <opaque_fragment>",
            "outgoingLight = mix(outgoingLight, diffuseColor.rgb, uFlat);\n#include <opaque_fragment>",
          );
      };
      material.customProgramCacheKey = () => "spellwake-dealt-card";
      const mesh = new THREE.Group();
      const face = new THREE.Mesh(cardShape, material);
      const reverse = new THREE.Mesh(cardShape, cardReverse);
      reverse.rotation.y = Math.PI;
      reverse.position.z = -CARD.thickness * 0.5;
      mesh.add(face, reverse);
      // First dealt is the top of the stack, nearest the viewer.
      mesh.position.set(0, -0.055, (count / 2 - index) * CARD.thickness * 0.95);
      mesh.rotation.z = Math.sin(index * 91.7) * 0.5 * 0.02;
      stack.add(mesh);
      const shadow = new THREE.Mesh(
        shadowGeometry,
        track(
          new THREE.MeshBasicMaterial({
            map: shadowTexture,
            transparent: true,
            depthWrite: false,
            opacity: 0,
            toneMapped: false,
          }),
        ),
      );
      shadow.visible = false;
      shadow.renderOrder = -1;
      scene.add(shadow);
      return {
        mesh,
        uniform,
        shadow,
        start: null,
        target: null,
        wobble: Math.sin(index * 37.3),
      };
    });
  };

  // ─── State ────────────────────────────────────────────────────────────────
  let disposed = false;
  let size = { width: 1, height: 1 };
  let area: Rect | null = null;
  let frame = { x: 0, y: 0, scale: 1 };
  let from = options.from ?? null;
  let phase: "arrive" | "wait" | "open" | "done" = "arrive";
  let phaseTime = 0;
  let clock = 0;
  let script: OpeningScript | null = null;
  let pendingSlots: readonly Rect[] | null = null;
  /** How far the wrapper falls, in its own units, to leave the bottom of the stage. */
  let drop = 2.6;
  let settle: (() => void) | null = null;
  let landed: Promise<void> | null = null;
  const pointer = { x: 0, y: 0, tx: 0, ty: 0 };
  let idle = 1;
  let tear = { progress: 0, mouth: 0, charge: 0 };
  let tearDirty = true;
  let rimDirty = true;
  let lastSpawnX = -1;
  let animation = 0;
  let previous = performance.now();

  const frameArea = (): void => {
    const target = area ?? {
      x: 0,
      y: 0,
      width: size.width,
      height: size.height,
    };
    frame = framePack(target, size, { width: PACK.width, height: PACK.height });
  };

  const resize = (): void => {
    const rect = host.getBoundingClientRect();
    const next = {
      width: Math.max(1, rect.width),
      height: Math.max(1, rect.height),
    };
    const changed = next.width !== size.width || next.height !== size.height;
    size = next;
    renderer.setSize(size.width, size.height, false);
    camera.aspect = size.width / size.height;
    camera.updateProjectionMatrix();
    moteMaterial.uniforms.uScale!.value =
      (size.height * renderer.getPixelRatio()) /
      (2 * Math.tan(((CAMERA.fov / 2) * Math.PI) / 180));
    frameArea();
    // Slots measured for a different layout would deal to the wrong places.
    if (changed && phase === "open") finish();
  };
  const observer = new ResizeObserver(resize);
  observer.observe(host);
  resize();

  const tornAt = (x: number): number => {
    const reach = 0.34;
    const front = mix(
      -PACK.width / 2 - 0.08,
      PACK.width / 2 + reach,
      tear.progress,
    );
    return smoothstep(0, 1, (front - x) / reach);
  };

  /** The strip peels back about the tear line and lifts away from it. */
  const stripShape: Deform = (x, y, side, out) => {
    restShape(x, y, side, out);
    const amount = tornAt(x);
    if (amount <= 0) return out;
    const line = tearLine(x);
    const dy = y - line;
    const angle = -amount * (0.75 + 4.5 * dy);
    const cos = Math.cos(angle);
    const sin = Math.sin(angle);
    const z = out.z;
    out.y = line + dy * cos + z * sin + amount * 0.045;
    out.z = -dy * sin + z * cos + amount * 0.02;
    out.x = x + amount * 0.02;
    return out;
  };
  /** The body gapes open where it has torn. */
  const bodyShape: Deform = (x, y, side, out) => {
    restShape(x, y, side, out);
    if (tear.mouth <= 0) return out;
    const u = (2 * x) / PACK.width;
    const line = tearLine(x);
    const near = smoothstep(line - 0.34, line, y);
    out.z +=
      side * tear.mouth * PACK.mouth * near * near * (1 - u * u) * tornAt(x);
    return out;
  };

  const scratch = new THREE.Vector3();
  const updateRims = (): void => {
    for (const rim of rims) {
      const positions = rim.geometry.getAttribute(
        "position",
      ) as THREE.BufferAttribute;
      const colors = rim.geometry.getAttribute(
        "color",
      ) as THREE.BufferAttribute;
      const shape = rim.region === "body" ? bodyShape : stripShape;
      const inward = rim.region === "body" ? -1 : 1;
      for (let column = 0; column <= RIM_COLUMNS; column += 1) {
        const x = (column / RIM_COLUMNS - 0.5) * PACK.width * 0.995;
        const line = tearLine(x);
        const torn = tornAt(x);
        const glow = Math.max(torn, tear.charge * (1 - tear.progress));
        const fade = 1 - smoothstep(0.42, 0.5, Math.abs(x));
        for (let edge = 0; edge < 2; edge += 1) {
          const y = line + inward * edge * (0.006 + 0.01 * torn);
          shape(x, y, rim.side, scratch);
          scratch.z += rim.side * 0.0012;
          const index = column * 2 + edge;
          positions.setXYZ(index, scratch.x, scratch.y, scratch.z);
          const alpha = glow * fade * (edge === 0 ? 1 : 0.15);
          colors.setXYZW(index, warm.r, warm.g, warm.b, alpha);
        }
      }
      positions.needsUpdate = true;
      colors.needsUpdate = true;
    }
  };

  /** Where the mouth's gape stops reaching down the body, with room to spare. */
  const gapeFloor = PACK.tearY - 0.4;
  let bodyPosed = { mouth: 0, progress: 0 };
  const updateWrapper = (): void => {
    if (tearDirty) {
      tearDirty = false;
      rimDirty = true;
      for (const sheet of sheets.strip) sheet.update(stripShape);
      // The body only moves where the mouth gapes, and only once it does.
      if (
        tear.mouth !== bodyPosed.mouth ||
        (tear.mouth > 0 && tear.progress !== bodyPosed.progress)
      ) {
        bodyPosed = { mouth: tear.mouth, progress: tear.progress };
        for (const sheet of sheets.body) sheet.update(bodyShape, gapeFloor);
      }
    }
    if (rimDirty) {
      rimDirty = false;
      updateRims();
    }
  };

  const spawnFlecks = (time: number): void => {
    const reach = 0.34;
    const front =
      mix(-PACK.width / 2 - 0.08, PACK.width / 2 + reach, tear.progress) -
      reach * 0.5;
    if (front < -PACK.width / 2 || front > PACK.width / 2) return;
    while (lastSpawnX < front) {
      lastSpawnX += 0.012;
      if (flecks.length >= FLECKS) break;
      const r = Math.sin(lastSpawnX * 912.3) * 0.5 + 0.5;
      const side = r > 0.5 ? 1 : -1;
      flecks.push({
        born: time,
        life: 0.9 + r * 0.7,
        position: new THREE.Vector3(
          lastSpawnX,
          tearLine(lastSpawnX),
          side * thickness(lastSpawnX, PACK.tearY),
        ),
        velocity: new THREE.Vector3(
          0.15 + r * 0.55,
          0.55 + Math.abs(Math.sin(lastSpawnX * 77)) * 0.9,
          side * (0.25 + r * 0.5),
        ),
        spin: new THREE.Vector3(8 + r * 14, 5 + r * 9, 3 + r * 6),
        rotation: new THREE.Euler(r * 6, r * 3, r * 2),
        size: 0.6 + r * 1.1,
      });
    }
  };

  const fleckMatrix = new THREE.Matrix4();
  const fleckQuaternion = new THREE.Quaternion();
  const fleckScale = new THREE.Vector3();
  const updateFlecks = (time: number, dt: number): void => {
    let live = 0;
    for (const fleck of flecks) {
      const age = time - fleck.born;
      if (age > fleck.life) continue;
      fleck.velocity.y -= 2.1 * dt;
      fleck.velocity.multiplyScalar(Math.exp(-1.6 * dt));
      fleck.position.addScaledVector(fleck.velocity, dt);
      fleck.rotation.x += fleck.spin.x * dt;
      fleck.rotation.y += fleck.spin.y * dt;
      fleck.rotation.z += fleck.spin.z * dt;
      const shrink = 1 - smoothstep(0.6, 1, age / fleck.life);
      fleckQuaternion.setFromEuler(fleck.rotation);
      fleckScale.setScalar(fleck.size * shrink);
      fleckMatrix.compose(fleck.position, fleckQuaternion, fleckScale);
      fleckMesh.setMatrixAt(live, fleckMatrix);
      live += 1;
    }
    fleckMesh.count = live;
    fleckMesh.instanceMatrix.needsUpdate = true;
  };

  const updateMotes = (time: number, dt: number, emit: number): void => {
    if (emit > 0 && motes.length < MOTES * 3) {
      const wanted = emit * dt * (detail === "full" ? 90 : 55);
      for (let index = 0; index < wanted; index += 1) {
        const seed = Math.random();
        motes.push({
          born: time,
          life: 1.1 + seed * 1.3,
          x: (Math.random() - 0.5) * PACK.width * 0.8,
          y: PACK.tearY + 0.02,
          z: (Math.random() - 0.5) * 0.1,
          drift: (Math.random() - 0.5) * 0.5,
          rise: 0.35 + Math.random() * 0.7,
          size: 0.012 + seed * 0.03,
        });
      }
    }
    const positions = moteGeometry.getAttribute(
      "position",
    ) as THREE.BufferAttribute;
    const alphas = moteGeometry.getAttribute("alpha") as THREE.BufferAttribute;
    const sizes = moteGeometry.getAttribute("size") as THREE.BufferAttribute;
    let live = 0;
    for (let index = motes.length - 1; index >= 0; index -= 1) {
      const mote = motes[index];
      if (!mote) continue;
      const age = time - mote.born;
      if (age > mote.life) {
        motes.splice(index, 1);
        continue;
      }
      if (live >= MOTES) continue;
      const t = age / mote.life;
      const sway = Math.sin(age * 3.1 + mote.drift * 9) * 0.04;
      positions.setXYZ(
        live,
        mote.x + mote.drift * age * 0.5 + sway,
        mote.y + mote.rise * age,
        mote.z,
      );
      alphas.setX(
        live,
        Math.sin(Math.PI * t) *
          (0.65 + 0.35 * Math.sin(age * 19 + mote.drift * 40)),
      );
      sizes.setX(live, mote.size);
      live += 1;
    }
    moteGeometry.setDrawRange(0, live);
    positions.needsUpdate = true;
    alphas.needsUpdate = true;
    sizes.needsUpdate = true;
  };

  const quaternion = new THREE.Quaternion();
  const euler = new THREE.Euler();
  const targetQuaternion = new THREE.Quaternion();

  const poseCard = (card: Card, k: number): void => {
    if (!card.start || !card.target) return;
    const e = ease.arrive(k);
    const { position, quaternion: startQuaternion, scale } = card.start;
    const target = card.target;
    const lift = Math.sin(Math.PI * Math.min(1, k * 1.05)) * 0.55;
    card.mesh.position.set(
      mix(position.x, target.x, e),
      mix(position.y, target.y, e),
      mix(position.z, 0, e) + lift * (1 - e * 0.4),
    );
    const dx = target.x - position.x;
    euler.set(
      -0.18 * Math.sin(Math.PI * k),
      0.32 *
        Math.sin(Math.PI * k) *
        Math.sign(dx || 1) *
        Math.min(1, Math.abs(dx)),
      card.wobble * 0.14 * (1 - e),
    );
    targetQuaternion.setFromEuler(euler);
    quaternion
      .copy(startQuaternion)
      .slerp(targetQuaternion, Math.min(1, e * 1.4));
    if (k >= 1) quaternion.identity();
    card.mesh.quaternion.copy(quaternion);
    card.mesh.scale.set(
      mix(scale.x, target.width / CARD.width, e),
      mix(scale.y, target.height / CARD.height, e),
      mix(scale.z, target.width / CARD.width, e),
    );
    card.uniform.value = smoothstep(0.6, 1, k);
    const k12 = 12 * worldPerPixel(size.height);
    card.shadow.visible = k > 0;
    card.shadow.position.set(
      card.mesh.position.x,
      card.mesh.position.y - k12 * e - lift * 0.18,
      -0.004,
    );
    card.shadow.scale.set(
      card.mesh.scale.x * CARD.width * (1 + 112 / 160),
      card.mesh.scale.y * CARD.height * (1 + 112 / 223),
      1,
    );
    card.shadow.material.opacity = smoothstep(0.15, 1, k) * (0.6 + 0.4 * e);
  };

  const startOpening = (slots: readonly Rect[]): void => {
    if (!script) {
      if (cards.length !== slots.length) {
        for (const card of cards) {
          card.mesh.removeFromParent();
          card.shadow.removeFromParent();
        }
        buildCards(slots.length);
      }
      script = openingScript(options.beats, slots.length);
    }
    const below = frame.y + (size.height * worldPerPixel(size.height)) / 2;
    drop = below / frame.scale + PACK.height * 0.75;
    cards.forEach((card, index) => {
      const slot = slots[index];
      card.target = slot ? placeRect(slot, size) : null;
    });
    phase = "open";
    phaseTime = 0;
    lastSpawnX = -PACK.width / 2;
  };

  const finish = (): void => {
    if (disposed || phase === "done") return;
    if (phase !== "open" && pendingSlots) startOpening(pendingSlots);
    phase = "done";
    rig.visible = false;
    fleckMesh.count = 0;
    moteGeometry.setDrawRange(0, 0);
    for (const card of cards) {
      if (card.mesh.parent !== scene) scene.attach(card.mesh);
      if (!card.start)
        card.start = {
          position: card.mesh.position.clone(),
          quaternion: card.mesh.quaternion.clone(),
          scale: card.mesh.scale.clone(),
        };
      poseCard(card, 1);
    }
    camera.position.set(0, 0, CAMERA.distance);
    renderer.render(scene, camera);
    settle?.();
    settle = null;
  };

  const tick = (now: number): void => {
    animation = 0;
    if (disposed) return;
    const dt = Math.min(0.05, (now - previous) / 1000);
    previous = now;
    clock += dt;
    phaseTime += dt;
    pointer.x += (pointer.tx - pointer.x) * (1 - Math.exp(-6 * dt));
    pointer.y += (pointer.ty - pointer.y) * (1 - Math.exp(-6 * dt));

    // Placement on the page: the lift off the shelf, then the framed area.
    let lift = 1;
    if (phase === "arrive") {
      lift = clamp01(phaseTime / options.beats.hero);
      if (lift >= 1) {
        phase = "wait";
        phaseTime = 0;
        from = null;
      }
    }
    const e = ease.arrive(lift);
    const start = from
      ? framePack(from, size, { width: PACK.width, height: PACK.height }, 1, 1)
      : {
          x: frame.x,
          y: frame.y - frame.scale * 0.9,
          scale: frame.scale * 0.8,
        };
    root.position.set(mix(start.x, frame.x, e), mix(start.y, frame.y, e), 0);
    root.scale.setScalar(mix(start.scale, frame.scale, e));

    let rigY = 0;
    let rigZ = Math.sin(Math.PI * e) * 0.6;
    let pitch = Math.sin(Math.PI * e) * 0.16;
    let yaw = (from ? 0 : (1 - e) * -0.5) + Math.sin(Math.PI * e) * -0.2;
    let squash = 1;
    let light = 0;
    let haloLevel = 0.32 * e;
    let emit = 0;

    if (phase === "wait" || phase === "arrive") {
      tear.charge =
        (phase === "wait" ? smoothstep(0, 1, phaseTime) : 0) *
        (0.28 + 0.14 * Math.sin(clock * 2.6));
      rimDirty = true;
      // Once the cards are known the pack steadies itself, then tears.
      if (phase === "wait" && pendingSlots) {
        idle = Math.max(0, idle - dt / options.beats.scene);
        if (idle === 0 && phaseTime > options.beats.scene)
          startOpening(pendingSlots);
      }
    }

    if (phase === "open" && script) {
      const time = phaseTime;
      const s = script;
      const anticipation = Math.sin(
        Math.PI * span(time, s.anticipation.start, s.anticipation.length),
      );
      rigY -= 0.03 * anticipation;
      pitch -= 0.06 * anticipation;
      squash = 1 - 0.018 * anticipation;
      const progress = ease.travel(span(time, s.tear.start, s.tear.length));
      const mouth = ease.arrive(span(time, s.mouth.start, s.mouth.length));
      if (progress !== tear.progress || mouth !== tear.mouth) tearDirty = true;
      tear = { progress, mouth, charge: tear.charge * (1 - progress) };
      rimDirty = true;
      spawnFlecks(clock);

      // The strip breaks free and is thrown up and away.
      const release = ease.depart(
        span(time, s.release.start, s.release.length),
      );
      strip.visible = release < 1;
      strip.position.set(
        release * 0.9,
        PACK.tearY + release * 1.35,
        -release * 0.9,
      );
      strip.rotation.set(-release * 1.6, release * 0.5, -release * 0.9);

      const burst = span(time, s.burst.start, s.burst.length);
      const exit = ease.depart(span(time, s.exit.start, s.exit.length));
      light =
        Math.pow(progress, 1.3) *
        (1 + 0.7 * Math.sin(Math.PI * Math.min(1, burst * 1.4))) *
        (1 - exit);
      emit = smoothstep(0.3, 1, progress) * (1 - exit);
      haloLevel = (0.32 + 0.22 * Math.sin(Math.PI * burst)) * (1 - exit);
      rigY -= exit * drop;
      rigZ -= exit * 0.8;
      pitch += exit * 0.22;

      // The cards rise out of the mouth while the wrapper falls away.
      const rise = ease.arrive(span(time, s.rise.start, s.rise.length));
      if (time >= s.rise.start && stack.parent !== scene) {
        root.updateMatrixWorld(true);
        scene.attach(stack);
        stack.userData.origin = stack.position.clone();
        stack.userData.scale = root.scale.x;
      }
      if (stack.parent === scene) {
        const origin = stack.userData.origin as THREE.Vector3;
        const unit = stack.userData.scale as number;
        const clear = smoothstep(0.35, 1, exit);
        stack.position.set(
          origin.x,
          origin.y + rise * 0.5 * unit,
          origin.z + clear * 0.3 * unit,
        );
        // Tilting inside the wrapper would push the cards through its face.
        stack.rotation.x = -0.1 * Math.sin(Math.PI * rise) * clear;
      }
      // Deal.
      cards.forEach((card, index) => {
        const begin = s.deal.start + index * s.deal.stagger;
        if (time < begin) return;
        if (!card.start) {
          stack.updateMatrixWorld(true);
          scene.attach(card.mesh);
          card.start = {
            position: card.mesh.position.clone(),
            quaternion: card.mesh.quaternion.clone(),
            scale: card.mesh.scale.clone(),
          };
        }
        poseCard(card, span(time, begin, s.deal.each));
      });
      // A gentle push in on the tear, back to rest before the deal lands.
      const push = Math.sin(Math.PI * ease.travel(span(time, 0, s.deal.start)));
      camera.position.z = CAMERA.distance - 0.5 * push;
      if (time >= s.end) {
        finish();
        return;
      }
    }

    // Breathing and following the pointer, faded out once the tear begins.
    const breathe = ease.travel(idle) * (phase === "arrive" ? e : 1);
    rigY += breathe * 0.022 * Math.sin(clock * 1.3);
    yaw += breathe * (0.07 * Math.sin(clock * 0.83) + pointer.x * 0.3);
    pitch += breathe * (0.035 * Math.sin(clock * 1.07) - pointer.y * 0.18);
    rig.position.set(0, rigY, rigZ);
    rig.rotation.set(pitch, yaw, breathe * 0.02 * Math.sin(clock * 0.6));
    rig.scale.set(1 / squash, squash, 1);

    updateWrapper();
    halo.material.opacity = haloLevel * (0.85 + 0.15 * Math.sin(clock * 2.6));
    mouthLight.intensity = light * 3.2;
    mouthGlow.material.opacity = Math.min(0.9, light * 0.8);
    mouthGlow.scale.set(0.9 + light * 0.25, 0.12 + light * 0.1, 1);
    for (const ray of rays) {
      const flicker = 0.75 + 0.25 * Math.sin(clock * 2.3 + ray.phase);
      ray.mesh.material.opacity = Math.min(1, light * ray.base * flicker);
      ray.mesh.rotation.y = Math.sin(clock * 0.7 + ray.phase) * 0.2;
    }
    updateFlecks(clock, dt);
    updateMotes(clock, dt, emit);

    renderer.render(scene, camera);
    if (host.dataset.phase !== phase) host.dataset.phase = phase;
    if (!document.hidden) animation = requestAnimationFrame(tick);
  };

  const pointerMove = (event: PointerEvent): void => {
    if (event.pointerType === "touch") return;
    pointer.tx = clamp01(event.clientX / window.innerWidth) * 2 - 1;
    pointer.ty = clamp01(event.clientY / window.innerHeight) * 2 - 1;
  };
  const visibility = (): void => {
    if (!document.hidden && !animation && phase !== "done") {
      previous = performance.now();
      animation = requestAnimationFrame(tick);
    }
  };
  const contextLost = (event: Event): void => {
    event.preventDefault();
    finish();
  };
  window.addEventListener("pointermove", pointerMove);
  document.addEventListener("visibilitychange", visibility);
  canvas.addEventListener("webglcontextlost", contextLost);
  // A pack's usual count waits inside, so its shaders compile with the rest.
  buildCards(14);
  // Compile every material before the first frame, so the lift is smooth.
  // Card shadows stay hidden until the deal; show them for compiling only.
  updateWrapper();
  for (const card of cards) card.shadow.visible = true;
  await renderer.compileAsync(scene, camera);
  for (const card of cards) card.shadow.visible = false;
  // Upload every texture now too, so no frame of the opening waits on one.
  scene.traverse((object) => {
    if (!(object instanceof THREE.Mesh || object instanceof THREE.Points))
      return;
    for (const material of [object.material].flat())
      for (const value of Object.values(material))
        if (value instanceof THREE.Texture) renderer.initTexture(value);
  });
  previous = performance.now();
  animation = requestAnimationFrame(tick);

  function dispose(): void {
    if (disposed) return;
    disposed = true;
    cancelAnimationFrame(animation);
    observer.disconnect();
    window.removeEventListener("pointermove", pointerMove);
    document.removeEventListener("visibilitychange", visibility);
    canvas.removeEventListener("webglcontextlost", contextLost);
    for (const value of disposables) value.dispose();
    printed.dispose();
    fleckMesh.dispose();
    environment.dispose();
    renderer.dispose();
    canvas.remove();
    settle?.();
    settle = null;
  }
  options.signal?.addEventListener("abort", dispose, { once: true });

  return {
    setArea(next) {
      // Once the tear starts, the cards are aimed from where the pack is.
      if (phase === "open" || phase === "done") return;
      area = next;
      frameArea();
    },
    open(slots) {
      if (landed) return landed;
      landed = new Promise<void>((resolve) => {
        settle = resolve;
      });
      if (phase === "done" || disposed) {
        settle?.();
        return landed;
      }
      pendingSlots = slots;
      return landed;
    },
    finish,
    dispose,
  };
}
