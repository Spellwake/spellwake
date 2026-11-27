/**
 * Live Three.js 3D match tabletop renderer connected to accepted PlayerSnapshot layouts.
 *
 * Uses only publication-cleared vector sleeves plus dynamic card face URLs supplied by
 * the host (falling back to a programmatic text-card CanvasTexture when art is absent).
 * Never loads retired study card scans or unapproved reference imagery.
 */
import * as THREE from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import {
  CARD_HEIGHT,
  CARD_THICKNESS_SCALE,
  CARD_WIDTH,
  createCardSilhouette,
  createCardStockGeometry,
  createDeckLayerGeometry,
  createMappedFaceGeometry,
  createRimGeometry,
  createSoftSleeveGeometries,
  SLEEVE_HEIGHT,
  SLEEVE_WIDTH,
} from "./card-geometry";
import { createSceneLifecycle } from "./scene-lifecycle";

export type LiveCardRim =
  | "playable"
  | "offered"
  | "teal"
  | "eligible"
  | "copper"
  | "ivory"
  | "gold";

export type LiveCardFallback = {
  name: string;
  typeLine?: string | undefined;
  rulesText?: string | undefined;
  stats?: string | undefined;
};

export type LiveSceneCard = {
  /** Stable identity across views so cards glide smoothly instead of respawning. */
  key: string;
  kind: "card" | "back" | "library" | "graveyard";
  zone: "battlefield" | "hand" | "pile";
  /** Center X in stage CSS pixels from left. */
  x: number;
  /** Center Y in stage CSS pixels from top. */
  y: number;
  /** Target width in stage CSS pixels. */
  width: number;
  /** Clockwise rotation in degrees (90 for tapped, small lean for hand fan). */
  rotateDeg: number;
  /** Monotonic layering index for overlapping cards. */
  order: number;
  /** Pile depth for 3D library/graveyard stacks. */
  count?: number | undefined;
  /** Dynamic card face image URL from catalogue/Scryfall, loaded on demand. */
  faceUrl?: string | undefined;
  /** Text fallback when faceUrl is absent or unavailable. */
  fallback?: LiveCardFallback | undefined;
  /** Active interaction highlight rim rendered directly in 3D. */
  rim?: LiveCardRim | null | undefined;
};

export type LiveMatchSceneState = {
  stage: { width: number; height: number };
  tilted: boolean;
  cards: readonly LiveSceneCard[];
  hoveredKey: string | null;
  reducedMotion: boolean;
};

export type ProjectedCardRect = {
  left: number;
  top: number;
  width: number;
  height: number;
  centerX: number;
  centerY: number;
};

export type LiveMatchSceneOptions = {
  /** Sleeve image URL. Remote textures must allow cross-origin access. */
  sleeveUrl: string;
  /** Use this texture if the selected sleeve cannot be loaded. */
  sleeveFallbackUrl?: string;
  /** Emitted whenever rendered 3D card screen projections change. */
  onProject?: (rects: ReadonlyMap<string, ProjectedCardRect>) => void;
  /** Optional abort signal bound to host lifecycle. */
  signal?: AbortSignal;
};

export type LiveMatchSceneHandle = {
  update(state: LiveMatchSceneState): void;
  dispose(): void;
  readonly disposed: boolean;
};

const FOV_DEG = 32;
const WORLD_TABLE_HEIGHT = 32;
const HAND_DEPTH = 24;
const PITCH_OVERVIEW = 0.025;
const PITCH_SEAT = 0.28;
const MAX_STACK_PLATES = 56;
const STACK_LAYER_STEP = 0.022 * CARD_THICKNESS_SCALE;

const RIM_COLORS: Record<LiveCardRim, number> = {
  playable: 0x7fe9e0,
  offered: 0x7fe9e0,
  teal: 0x7fe9e0,
  eligible: 0xf4ead2,
  copper: 0xee9a63,
  ivory: 0xf4ead2,
  gold: 0xe7cf9f,
};

/** Pure helper: compute camera distance for the table plane at `y = 0`. */
export function tableCameraDistance(fovDeg = FOV_DEG): number {
  return WORLD_TABLE_HEIGHT / 2 / Math.tan(THREE.MathUtils.degToRad(fovDeg / 2));
}

/** Pure helper: map a stage CSS pixel placement onto world coordinates at `y = 0`. */
export function stageToTableWorld(
  px: number,
  py: number,
  cardWidthPx: number,
  stage: { width: number; height: number },
): { x: number; z: number; scale: number } {
  const safeW = Math.max(1, stage.width);
  const safeH = Math.max(1, stage.height);
  const aspect = safeW / safeH;
  const worldW = WORLD_TABLE_HEIGHT * aspect;
  return {
    x: (px / safeW - 0.5) * worldW,
    z: (py / safeH - 0.5) * WORLD_TABLE_HEIGHT,
    scale: (cardWidthPx / safeH) * (WORLD_TABLE_HEIGHT / CARD_WIDTH),
  };
}

/** Pure helper: map a stage CSS pixel placement into camera-local hand coordinates. */
export function stageToHandLocal(
  px: number,
  py: number,
  cardWidthPx: number,
  stage: { width: number; height: number },
  fovDeg = FOV_DEG,
  depth = HAND_DEPTH,
): { x: number; y: number; scale: number } {
  const safeW = Math.max(1, stage.width);
  const safeH = Math.max(1, stage.height);
  const aspect = safeW / safeH;
  const handH = 2 * depth * Math.tan(THREE.MathUtils.degToRad(fovDeg / 2));
  const handW = handH * aspect;
  return {
    x: (px / safeW - 0.5) * handW,
    y: (0.5 - py / safeH) * handH,
    scale: (cardWidthPx / safeH) * (handH / CARD_WIDTH),
  };
}

/** Pure helper: compute target quaternion for a card on the table or in hand. */
export function cardTargetQuaternion(
  zone: LiveSceneCard["zone"],
  faceDown: boolean,
  rotateDeg: number,
): THREE.Quaternion {
  const qFlip = faceDown
    ? new THREE.Quaternion().setFromAxisAngle(
        new THREE.Vector3(0, 1, 0),
        Math.PI,
      )
    : new THREE.Quaternion();
  const qTurn = new THREE.Quaternion().setFromAxisAngle(
    new THREE.Vector3(0, 0, 1),
    THREE.MathUtils.degToRad(-rotateDeg),
  );
  if (zone === "hand") {
    const qTilt = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(1, 0, 0),
      -0.08,
    );
    return qTilt.multiply(qTurn).multiply(qFlip);
  }
  const qFlat = new THREE.Quaternion().setFromAxisAngle(
    new THREE.Vector3(1, 0, 0),
    -Math.PI / 2,
  );
  return qFlat.multiply(qTurn).multiply(qFlip);
}

/** Deterministic subtle per-layer jitter for instanced 3D deck stacks. */
function stackJitter(index: number): { dx: number; dz: number; rot: number } {
  const a = Math.sin(index * 12.9898 + 1.23) * 43758.5453;
  const b = Math.sin(index * 78.233 + 4.56) * 43758.5453;
  const c = Math.sin(index * 37.719 + 7.89) * 43758.5453;
  const frac = (n: number) => n - Math.floor(n) - 0.5;
  return {
    dx: frac(a) * 0.028,
    dz: frac(b) * 0.028,
    rot: frac(c) * 0.018,
  };
}

/** Procedural woven green-slate felt texture drawn in code (no external raster file). */
function createFeltTexture(): THREE.CanvasTexture {
  const size = 512;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (ctx) {
    const gradient = ctx.createRadialGradient(
      size * 0.5,
      size * 0.42,
      size * 0.04,
      size * 0.5,
      size * 0.5,
      size * 0.72,
    );
    gradient.addColorStop(0, "#506149");
    gradient.addColorStop(0.52, "#42503e");
    gradient.addColorStop(1, "#303d31");
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, size, size);

    // Subtle woven felt grain.
    const image = ctx.getImageData(0, 0, size, size);
    const data = image.data;
    let seed = 7319;
    for (let i = 0; i < data.length; i += 4) {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      const noise = ((seed >>> 24) / 255 - 0.5) * 10;
      data[i] = Math.min(255, Math.max(0, (data[i] ?? 0) + noise));
      data[i + 1] = Math.min(255, Math.max(0, (data[i + 1] ?? 0) + noise));
      data[i + 2] = Math.min(255, Math.max(0, (data[i + 2] ?? 0) + noise));
    }
    ctx.putImageData(image, 0, 0);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.wrapS = THREE.ClampToEdgeWrapping;
  texture.wrapT = THREE.ClampToEdgeWrapping;
  return texture;
}

/** Programmatic card-face fallback texture when no printing image is available. */
function createFallbackCardTexture(fallback: LiveCardFallback): THREE.CanvasTexture {
  const width = 400;
  const height = 560;
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (ctx) {
    ctx.fillStyle = "#0b0d11";
    ctx.fillRect(0, 0, width, height);
    const grad = ctx.createLinearGradient(0, 14, 0, height - 14);
    grad.addColorStop(0, "#efe6cf");
    grad.addColorStop(1, "#ddd2b6");
    ctx.fillStyle = grad;
    ctx.fillRect(14, 14, width - 28, height - 28);

    ctx.fillStyle = "#1b1710";
    ctx.font = "600 28px Georgia, serif";
    ctx.fillText(fallback.name.slice(0, 26), 28, 56, width - 56);

    if (fallback.typeLine) {
      ctx.fillStyle = "#4d4536";
      ctx.font = "500 18px system-ui, sans-serif";
      ctx.fillText(fallback.typeLine.slice(0, 38), 28, 92, width - 56);
    }

    if (fallback.rulesText) {
      ctx.fillStyle = "#2b251c";
      ctx.font = "400 18px system-ui, sans-serif";
      const words = fallback.rulesText.split(/\s+/);
      let line = "";
      let y = 148;
      for (const word of words) {
        const next = line ? `${line} ${word}` : word;
        if (ctx.measureText(next).width > width - 56 && line) {
          ctx.fillText(line, 28, y, width - 56);
          line = word;
          y += 25;
          if (y > height - 76) break;
        } else {
          line = next;
        }
      }
      if (line && y <= height - 76) ctx.fillText(line, 28, y, width - 56);
    }

    if (fallback.stats) {
      ctx.fillStyle = "#1b1710";
      ctx.font = "700 26px system-ui, sans-serif";
      ctx.textAlign = "right";
      ctx.fillText(fallback.stats, width - 28, height - 30);
    }
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

type CardActor = {
  key: string;
  zone: LiveSceneCard["zone"];
  group: THREE.Group;
  faceMesh: THREE.Mesh;
  faceMaterial: THREE.MeshPhysicalMaterial;
  rimMesh: THREE.Mesh;
  rimMaterial: THREE.MeshBasicMaterial;
  stackMesh: THREE.InstancedMesh | null;
  stackCount: number;
  stackFaceDown: boolean;
  faceSourceKey: string;
  targetPosition: THREE.Vector3;
  targetQuaternion: THREE.Quaternion;
  targetScale: number;
  currentScale: number;
  faceDown: boolean;
  cardWidth: number;
  cardHeight: number;
  initialized: boolean;
};

/**
 * Mount the live Three.js 3D match tabletop onto `canvas`.
 * Rendering is strictly on-demand: once camera, cards, hover lift, and textures
 * settle, the animation loop pauses until the next state update or resize.
 */
export function mountLiveMatchScene(
  canvas: HTMLCanvasElement,
  options: LiveMatchSceneOptions,
): LiveMatchSceneHandle {
  const lifetime = createSceneLifecycle(options.signal);
  const ownedGeometries = new Set<THREE.BufferGeometry>();
  const ownedMaterials = new Set<THREE.Material>();
  const ownedTextures = new Set<THREE.Texture>();

  const ownGeo = <T extends THREE.BufferGeometry>(g: T): T => {
    ownedGeometries.add(g);
    return g;
  };
  const ownMat = <T extends THREE.Material>(m: T): T => {
    ownedMaterials.add(m);
    return m;
  };
  const ownTex = <T extends THREE.Texture>(t: T): T => {
    ownedTextures.add(t);
    return t;
  };

  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: true,
    powerPreference: "high-performance",
  });
  renderer.setPixelRatio(Math.min(2, Math.max(window.devicePixelRatio || 1, 1)));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 0.9;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x283329);

  // Subtle studio environment reflection (same RoomEnvironment recipe as approved card study).
  const pmrem = new THREE.PMREMGenerator(renderer);
  const roomEnv = new RoomEnvironment();
  const envTarget = pmrem.fromScene(roomEnv, 0.04);
  scene.environment = envTarget.texture;
  scene.environmentIntensity = 0.34;
  roomEnv.dispose();
  pmrem.dispose();

  const camera = new THREE.PerspectiveCamera(FOV_DEG, 1, 1, 220);
  const handGroup = new THREE.Group();
  camera.add(handGroup);
  scene.add(camera);

  const boardGroup = new THREE.Group();
  scene.add(boardGroup);

  // Studio tabletop lighting tuned so card art and rules text stay crisp without washout.
  scene.add(new THREE.HemisphereLight(0xfff7e7, 0x323d35, 0.85));
  const keyLight = new THREE.DirectionalLight(0xfff1d6, 1.05);
  keyLight.position.set(-8, 42, 16);
  keyLight.castShadow = true;
  keyLight.shadow.mapSize.set(2048, 2048);
  keyLight.shadow.camera.left = -36;
  keyLight.shadow.camera.right = 36;
  keyLight.shadow.camera.top = 26;
  keyLight.shadow.camera.bottom = -26;
  keyLight.shadow.camera.near = 10;
  keyLight.shadow.camera.far = 90;
  keyLight.shadow.bias = -0.0003;
  keyLight.shadow.normalBias = 0.02;
  keyLight.shadow.radius = 2.5;
  scene.add(keyLight);

  const fillLight = new THREE.DirectionalLight(0xc8dcf0, 0.32);
  fillLight.position.set(14, 22, -18);
  scene.add(fillLight);

  // Procedural felt tabletop surface.
  const feltTexture = ownTex(createFeltTexture());
  const tableGeo = ownGeo(new THREE.PlaneGeometry(180, 120));
  const tableMat = ownMat(
    new THREE.MeshStandardMaterial({
      map: feltTexture,
      roughness: 0.94,
      metalness: 0.0,
    }),
  );
  const tableMesh = new THREE.Mesh(tableGeo, tableMat);
  tableMesh.rotation.x = -Math.PI / 2;
  tableMesh.position.y = 0;
  tableMesh.receiveShadow = true;
  scene.add(tableMesh);

  // Shared approved card, pressed soft-sleeve, rim, and instanced deck-layer geometries.
  const cardShape = createCardSilhouette();
  const faceGeo = ownGeo(
    createMappedFaceGeometry(cardShape, CARD_WIDTH, CARD_HEIGHT),
  );
  const stockGeo = ownGeo(createCardStockGeometry(cardShape));
  const sleeveGeos = createSoftSleeveGeometries();
  ownGeo(sleeveGeos.backFace);
  ownGeo(sleeveGeos.stock);
  ownGeo(sleeveGeos.seam);
  ownGeo(sleeveGeos.frontFilm);
  const deckLayerGeo = ownGeo(createDeckLayerGeometry());
  const rimGeo = ownGeo(createRimGeometry(0.085));

  // Shared materials.
  const stockMat = ownMat(
    new THREE.MeshStandardMaterial({ color: 0x8c8a7d, roughness: 0.92 }),
  );
  const sleeveStockMat = ownMat(
    new THREE.MeshStandardMaterial({ color: 0x182233, roughness: 0.68 }),
  );
  const sleeveSeamMat = ownMat(
    new THREE.MeshPhysicalMaterial({
      color: 0x1d283a,
      roughness: 0.52,
      specularIntensity: 0.35,
      side: THREE.DoubleSide,
    }),
  );
  const sleeveFrontMat = ownMat(
    new THREE.MeshPhysicalMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 0.035,
      roughness: 0.22,
      clearcoat: 0.22,
      clearcoatRoughness: 0.24,
      depthWrite: false,
    }),
  );
  const deckLayerMat = ownMat(
    new THREE.MeshStandardMaterial({ color: 0x202c3f, roughness: 0.72 }),
  );

  const textureLoader = new THREE.TextureLoader();
  textureLoader.setCrossOrigin("anonymous");
  const maxAnisotropy = Math.min(8, renderer.capabilities.getMaxAnisotropy());

  const sleeveBackMat = ownMat(
    new THREE.MeshPhysicalMaterial({
      color: 0x192538,
      roughness: 0.58,
      clearcoat: 0.12,
      clearcoatRoughness: 0.45,
      specularIntensity: 0.3,
      envMapIntensity: 0.25,
    }),
  );

  // Load the selected sleeve asynchronously.
  let dirty = true;
  let rafId = 0;
  let lastTick = 0;
  let currentPitch = PITCH_SEAT;
  let targetPitch = PITCH_SEAT;
  let currentDistScale = 1.04;
  let targetDistScale = 1.04;
  let currentStage = { width: 0, height: 0 };
  let reducedMotion = false;
  let latestState: LiveMatchSceneState | null = null;

  const urlTextures = new Map<string, THREE.Texture>();
  const urlPending = new Map<string, Promise<THREE.Texture | null>>();
  const fallbackTextures = new Map<string, THREE.Texture>();
  const actors = new Map<string, CardActor>();

  function scheduleRender(): void {
    if (lifetime.disposed) return;
    dirty = true;
    if (rafId !== 0) return;
    lastTick = performance.now();
    rafId = requestAnimationFrame(renderFrame);
  }

  // Rasterize the sleeve onto a HiDPI canvas texture
  // so WebGL2 mipmaps and anisotropic filtering work deterministically across browsers.
  const sleeveImg = new Image();
  sleeveImg.crossOrigin = "anonymous";
  sleeveImg.onload = () => {
    if (lifetime.disposed) return;
    const svgCanvas = document.createElement("canvas");
    svgCanvas.width = 600;
    svgCanvas.height = 840;
    const ctx = svgCanvas.getContext("2d");
    if (!ctx) return;
    ctx.drawImage(sleeveImg, 0, 0, svgCanvas.width, svgCanvas.height);
    const tex = ownTex(new THREE.CanvasTexture(svgCanvas));
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.anisotropy = maxAnisotropy;
    tex.needsUpdate = true;
    sleeveBackMat.color.setHex(0xffffff);
    sleeveBackMat.map = tex;
    sleeveBackMat.needsUpdate = true;
    scheduleRender();
  };
  let sleeveUrl = options.sleeveUrl;
  sleeveImg.onerror = () => {
    if (lifetime.disposed) return;
    if (options.sleeveFallbackUrl && options.sleeveFallbackUrl !== sleeveUrl) {
      console.warn(
        "Unable to load sleeve texture, using default sleeve",
        sleeveUrl,
      );
      sleeveUrl = options.sleeveFallbackUrl;
      sleeveImg.src = sleeveUrl;
      return;
    }
    if (!lifetime.disposed) {
      console.warn(
        "Unable to load sleeve texture, using solid fallback",
        sleeveUrl,
      );
    }
  };
  sleeveImg.src = sleeveUrl;

  function fallbackTextureFor(fallback: LiveCardFallback): THREE.Texture {
    const cacheKey = `${fallback.name}|${fallback.typeLine ?? ""}|${fallback.stats ?? ""}`;
    const existing = fallbackTextures.get(cacheKey);
    if (existing) return existing;
    const created = ownTex(createFallbackCardTexture(fallback));
    created.anisotropy = maxAnisotropy;
    fallbackTextures.set(cacheKey, created);
    return created;
  }

  function requestFaceTexture(url: string, actor: CardActor, sourceKey: string): void {
    const cached = urlTextures.get(url);
    if (cached) {
      actor.faceMaterial.map = cached;
      actor.faceMaterial.color.setHex(0xffffff);
      actor.faceMaterial.needsUpdate = true;
      scheduleRender();
      return;
    }
    let promise = urlPending.get(url);
    if (!promise) {
      promise = textureLoader
        .loadAsync(url)
        .then((tex) => {
          if (lifetime.disposed) {
            tex.dispose();
            return null;
          }
          tex.colorSpace = THREE.SRGBColorSpace;
          tex.anisotropy = maxAnisotropy;
          ownTex(tex);
          urlTextures.set(url, tex);
          return tex;
        })
        .catch(() => null);
      urlPending.set(url, promise);
    }
    void promise.then((tex) => {
      if (lifetime.disposed || !tex || actor.faceSourceKey !== sourceKey) return;
      actor.faceMaterial.map = tex;
      actor.faceMaterial.color.setHex(0xffffff);
      actor.faceMaterial.needsUpdate = true;
      scheduleRender();
    });
  }

  function createActor(card: LiveSceneCard): CardActor {
    const group = new THREE.Group();

    const stockMesh = new THREE.Mesh(stockGeo, stockMat);
    stockMesh.castShadow = card.zone !== "hand";
    stockMesh.receiveShadow = card.zone !== "hand";
    group.add(stockMesh);

    const sleeveStockMesh = new THREE.Mesh(sleeveGeos.stock, sleeveStockMat);
    sleeveStockMesh.castShadow = card.zone !== "hand";
    group.add(sleeveStockMesh);

    const sleeveSeamMesh = new THREE.Mesh(sleeveGeos.seam, sleeveSeamMat);
    group.add(sleeveSeamMesh);

    const sleeveBackMesh = new THREE.Mesh(sleeveGeos.backFace, sleeveBackMat);
    sleeveBackMesh.rotation.y = Math.PI;
    sleeveBackMesh.position.z = -0.0265 * CARD_THICKNESS_SCALE;
    group.add(sleeveBackMesh);

    const faceMaterial = ownMat(
      new THREE.MeshPhysicalMaterial({
        color: 0xd8d0be,
        roughness: 0.78,
        metalness: 0,
        specularIntensity: 0.1,
        clearcoat: 0.02,
        clearcoatRoughness: 0.6,
        envMapIntensity: 0.12,
      }),
    );
    const faceMesh = new THREE.Mesh(faceGeo, faceMaterial);
    faceMesh.position.z = 0.0096 * CARD_THICKNESS_SCALE;
    group.add(faceMesh);

    const frontFilmMesh = new THREE.Mesh(sleeveGeos.frontFilm, sleeveFrontMat);
    frontFilmMesh.position.z = 0.024 * CARD_THICKNESS_SCALE;
    group.add(frontFilmMesh);

    const rimMaterial = ownMat(
      new THREE.MeshBasicMaterial({
        color: RIM_COLORS.playable,
        transparent: true,
        opacity: 0.92,
        side: THREE.DoubleSide,
        toneMapped: false,
      }),
    );
    const rimMesh = new THREE.Mesh(rimGeo, rimMaterial);
    rimMesh.position.z = -0.022 * CARD_THICKNESS_SCALE;
    rimMesh.visible = false;
    group.add(rimMesh);

    return {
      key: card.key,
      zone: card.zone,
      group,
      faceMesh,
      faceMaterial,
      rimMesh,
      rimMaterial,
      stackMesh: null,
      stackCount: 0,
      stackFaceDown: false,
      faceSourceKey: "",
      targetPosition: new THREE.Vector3(),
      targetQuaternion: new THREE.Quaternion(),
      targetScale: 1,
      currentScale: 1,
      faceDown: card.kind === "back" || card.kind === "library",
      cardWidth: SLEEVE_WIDTH,
      cardHeight: SLEEVE_HEIGHT,
      initialized: false,
    };
  }

  function syncStack(actor: CardActor, card: LiveSceneCard): void {
    const desiredPlates =
      (card.kind === "library" || card.kind === "graveyard") &&
      (card.count ?? 1) > 1
        ? Math.min(Math.max(0, (card.count ?? 1) - 1), MAX_STACK_PLATES)
        : 0;
    const faceDown = card.kind === "back" || card.kind === "library";
    if (desiredPlates === actor.stackCount && faceDown === actor.stackFaceDown)
      return;

    if (actor.stackMesh) {
      actor.stackMesh.removeFromParent();
      actor.stackMesh.dispose();
      actor.stackMesh = null;
    }
    actor.stackCount = desiredPlates;
    actor.stackFaceDown = faceDown;
    if (desiredPlates <= 0) return;

    const inst = new THREE.InstancedMesh(
      deckLayerGeo,
      deckLayerMat,
      desiredPlates,
    );
    inst.castShadow = true;
    inst.receiveShadow = true;
    const dummy = new THREE.Object3D();
    // Face-up cards lay with +Z up, so stack plates extend into -Z (toward the table).
    // Face-down libraries flip 180° around Y, so -Z points up and stack plates extend into +Z.
    const stackDir = faceDown ? 1 : -1;
    for (let i = 0; i < desiredPlates; i++) {
      const j = stackJitter(i);
      dummy.position.set(
        j.dx,
        j.dz,
        stackDir * ((i + 1) * STACK_LAYER_STEP + 0.022),
      );
      dummy.rotation.set(0, 0, j.rot);
      dummy.updateMatrix();
      inst.setMatrixAt(i, dummy.matrix);
    }
    inst.instanceMatrix.needsUpdate = true;
    actor.group.add(inst);
    actor.stackMesh = inst;
  }

  function updateCameraTransform(pitch: number, distScale: number): void {
    const dist = tableCameraDistance(FOV_DEG) * distScale;
    camera.position.set(0, dist * Math.cos(pitch), dist * Math.sin(pitch));
    camera.up.set(0, 1, 0);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld(true);
  }

  function applyState(state: LiveMatchSceneState): void {
    latestState = state;
    reducedMotion = state.reducedMotion;
    const { width, height } = state.stage;
    if (width <= 0 || height <= 0) return;

    if (width !== currentStage.width || height !== currentStage.height) {
      currentStage = { width, height };
      renderer.setSize(width, height, false);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
    }

    targetPitch = state.tilted ? PITCH_SEAT : PITCH_OVERVIEW;
    targetDistScale = state.tilted ? 1.05 : 1.01;
    if (reducedMotion) {
      currentPitch = targetPitch;
      currentDistScale = targetDistScale;
    }
    updateCameraTransform(currentPitch, currentDistScale);

    const activeKeys = new Set<string>();
    const handCards = state.cards.filter((c) => c.zone === "hand");
    const handIndexByKey = new Map<string, number>();
    handCards.forEach((c, idx) => handIndexByKey.set(c.key, idx));

    for (const card of state.cards) {
      activeKeys.add(card.key);
      let actor = actors.get(card.key);
      if (!actor) {
        actor = createActor(card);
        actors.set(card.key, actor);
      }

      // Reparent if a card moved between hand and table.
      const desiredParent = card.zone === "hand" ? handGroup : boardGroup;
      if (actor.group.parent !== desiredParent) {
        desiredParent.add(actor.group);
        actor.zone = card.zone;
        actor.initialized = false;
      }

      const faceDown = card.kind === "back" || card.kind === "library";
      actor.faceDown = faceDown;
      actor.faceMesh.visible = !faceDown;

      if (!faceDown) {
        const nextSourceKey = card.faceUrl
          ? `url:${card.faceUrl}`
          : `text:${card.fallback?.name ?? ""}:${card.fallback?.stats ?? ""}`;
        if (actor.faceSourceKey !== nextSourceKey) {
          actor.faceSourceKey = nextSourceKey;
          if (card.fallback) {
            actor.faceMaterial.map = fallbackTextureFor(card.fallback);
            actor.faceMaterial.color.setHex(0xffffff);
            actor.faceMaterial.needsUpdate = true;
          }
          if (card.faceUrl) {
            requestFaceTexture(card.faceUrl, actor, nextSourceKey);
          }
        }
      }

      // Rim highlight in 3D.
      if (card.rim) {
        actor.rimMaterial.color.setHex(RIM_COLORS[card.rim]);
        actor.rimMesh.visible = true;
      } else {
        actor.rimMesh.visible = false;
      }

      syncStack(actor, card);

      const isHovered = state.hoveredKey === card.key;
      if (card.zone === "hand") {
        const mapped = stageToHandLocal(
          card.x,
          card.y,
          card.width,
          state.stage,
          FOV_DEG,
          HAND_DEPTH,
        );
        const handIdx = handIndexByKey.get(card.key) ?? 0;
        const liftY = isHovered ? CARD_HEIGHT * mapped.scale * 0.28 : 0;
        const liftZ = isHovered ? 1.15 : 0;
        const scaleMult = isHovered ? 1.12 : 1;
        const rotDeg = isHovered ? card.rotateDeg * 0.15 : card.rotateDeg;
        actor.targetPosition.set(
          mapped.x,
          mapped.y + liftY,
          -HAND_DEPTH + handIdx * 0.045 + liftZ,
        );
        actor.targetScale = mapped.scale * scaleMult;
        actor.targetQuaternion.copy(
          cardTargetQuaternion("hand", faceDown, rotDeg),
        );
      } else {
        const mapped = stageToTableWorld(
          card.x,
          card.y,
          card.width,
          state.stage,
        );
        const stackHeight = actor.stackCount * STACK_LAYER_STEP * mapped.scale;
        const hoverLift = isHovered ? 0.16 : 0;
        const layerY =
          0.035 + stackHeight + (card.order % 40) * 0.002 + hoverLift;
        actor.targetPosition.set(mapped.x, layerY, mapped.z);
        actor.targetScale = mapped.scale;
        actor.targetQuaternion.copy(
          cardTargetQuaternion(card.zone, faceDown, card.rotateDeg),
        );
      }

      if (!actor.initialized || reducedMotion) {
        actor.group.position.copy(actor.targetPosition);
        actor.group.quaternion.copy(actor.targetQuaternion);
        actor.currentScale = actor.targetScale;
        actor.group.scale.setScalar(actor.currentScale);
        actor.initialized = true;
      }
    }

    // Remove exited cards.
    for (const [key, actor] of actors) {
      if (activeKeys.has(key)) continue;
      actor.group.removeFromParent();
      if (actor.stackMesh) {
        actor.stackMesh.dispose();
        actor.stackMesh = null;
      }
      ownedMaterials.delete(actor.faceMaterial);
      actor.faceMaterial.dispose();
      ownedMaterials.delete(actor.rimMaterial);
      actor.rimMaterial.dispose();
      actors.delete(key);
    }

    scheduleRender();
  }

  // Reusable projection scratch vectors to avoid per-frame allocations.
  const corners = [
    new THREE.Vector3(),
    new THREE.Vector3(),
    new THREE.Vector3(),
    new THREE.Vector3(),
  ];

  function projectActors(): void {
    const onProject = options.onProject;
    if (!onProject || currentStage.width <= 0 || currentStage.height <= 0) {
      return;
    }
    const { width, height } = currentStage;
    const rects = new Map<string, ProjectedCardRect>();
    for (const [key, actor] of actors) {
      actor.group.updateWorldMatrix(true, false);
      const hw = actor.cardWidth / 2;
      const hh = actor.cardHeight / 2;
      const z = (actor.faceDown ? -0.0265 : 0.015) * CARD_THICKNESS_SCALE;
      corners[0]!.set(-hw, -hh, z);
      corners[1]!.set(hw, -hh, z);
      corners[2]!.set(hw, hh, z);
      corners[3]!.set(-hw, hh, z);

      let minX = Infinity;
      let maxX = -Infinity;
      let minY = Infinity;
      let maxY = -Infinity;
      for (const corner of corners) {
        corner.applyMatrix4(actor.group.matrixWorld).project(camera);
        const sx = (corner.x * 0.5 + 0.5) * width;
        const sy = (0.5 - corner.y * 0.5) * height;
        if (sx < minX) minX = sx;
        if (sx > maxX) maxX = sx;
        if (sy < minY) minY = sy;
        if (sy > maxY) maxY = sy;
      }
      if (!Number.isFinite(minX) || !Number.isFinite(maxX)) continue;
      const rectW = Math.max(1, maxX - minX);
      const rectH = Math.max(1, maxY - minY);
      rects.set(key, {
        left: minX,
        top: minY,
        width: rectW,
        height: rectH,
        centerX: (minX + maxX) / 2,
        centerY: (minY + maxY) / 2,
      });
    }
    onProject(rects);
  }

  function renderFrame(now: number): void {
    rafId = 0;
    if (lifetime.disposed) return;

    const dt = Math.min(0.05, Math.max(0.001, (now - lastTick) / 1000));
    lastTick = now;
    const alpha = reducedMotion ? 1 : 1 - Math.exp(-10.5 * dt);

    let animating = false;

    if (Math.abs(targetPitch - currentPitch) > 0.0005) {
      currentPitch = THREE.MathUtils.lerp(currentPitch, targetPitch, alpha);
      animating = true;
    } else {
      currentPitch = targetPitch;
    }
    if (Math.abs(targetDistScale - currentDistScale) > 0.0005) {
      currentDistScale = THREE.MathUtils.lerp(
        currentDistScale,
        targetDistScale,
        alpha,
      );
      animating = true;
    } else {
      currentDistScale = targetDistScale;
    }
    updateCameraTransform(currentPitch, currentDistScale);

    for (const actor of actors.values()) {
      if (actor.group.position.distanceToSquared(actor.targetPosition) > 1e-5) {
        actor.group.position.lerp(actor.targetPosition, alpha);
        animating = true;
      } else {
        actor.group.position.copy(actor.targetPosition);
      }

      if (actor.group.quaternion.angleTo(actor.targetQuaternion) > 0.001) {
        actor.group.quaternion.slerp(actor.targetQuaternion, alpha);
        animating = true;
      } else {
        actor.group.quaternion.copy(actor.targetQuaternion);
      }

      if (Math.abs(actor.targetScale - actor.currentScale) > 0.0005) {
        actor.currentScale = THREE.MathUtils.lerp(
          actor.currentScale,
          actor.targetScale,
          alpha,
        );
        actor.group.scale.setScalar(actor.currentScale);
        animating = true;
      } else if (actor.currentScale !== actor.targetScale) {
        actor.currentScale = actor.targetScale;
        actor.group.scale.setScalar(actor.currentScale);
      }
    }

    if (currentStage.width > 0 && currentStage.height > 0) {
      renderer.render(scene, camera);
      projectActors();
    }

    dirty = false;
    if (animating || dirty) {
      rafId = requestAnimationFrame(renderFrame);
    }
  }

  const onContextLost = (event: Event): void => {
    event.preventDefault();
    if (rafId !== 0) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
  };
  const onContextRestored = (): void => {
    if (latestState) applyState(latestState);
  };
  canvas.addEventListener("webglcontextlost", onContextLost, {
    signal: lifetime.signal,
  });
  canvas.addEventListener("webglcontextrestored", onContextRestored, {
    signal: lifetime.signal,
  });

  lifetime.add(() => {
    sleeveImg.onload = null;
    sleeveImg.onerror = null;
    if (rafId !== 0) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
    for (const actor of actors.values()) {
      actor.group.removeFromParent();
      if (actor.stackMesh) actor.stackMesh.dispose();
    }
    actors.clear();
    for (const geo of ownedGeometries) geo.dispose();
    for (const mat of ownedMaterials) mat.dispose();
    for (const tex of ownedTextures) tex.dispose();
    envTarget.dispose();
    keyLight.shadow.dispose();
    scene.clear();
    renderer.dispose();
  });

  return {
    update(state: LiveMatchSceneState): void {
      if (lifetime.disposed) return;
      applyState(state);
    },
    dispose(): void {
      lifetime.dispose();
    },
    get disposed(): boolean {
      return lifetime.disposed;
    },
  };
}
