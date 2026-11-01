/** Original card imagery on dimensional cardstock, outside timing fixtures. */
import * as THREE from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { z } from "zod";
import {
  createTableNavigation,
  navigationDirections,
} from "./table-navigation";
import { createTableOverview } from "./table-overview";
import { createTableCameraMotion } from "./table-camera";
import { createCardFacingFade } from "./card-facing";
import {
  createTableLayout,
  readableCardTurn,
  tableSeatAnchor,
} from "./table-layout";
import {
  createHandStudy,
  isHandLift,
  type PublicPlayerZones,
} from "./hand-study";
import { BOARD_SAMPLES } from "./classic-board";
import { createCardInspection } from "./card-inspection";
import { createPlayPreview } from "./play-preview";
import { createPileBrowser } from "./pile-browser";
import { createSceneLifecycle, sceneAssetUrl } from "./scene-lifecycle";
import {
  createTableInteraction,
  type TableDropIntent,
} from "./table-interaction";
import type { SceneSettings, SceneSettingId } from "./scene-settings";
import { createPressedSleeveSeam } from "./sleeve-profile";

// With the 2.5-inch card width, printed faces sit about 0.28 mm apart.
const CARD_THICKNESS_SCALE = 0.6;

export type CardSceneSurface = {
  canvas: HTMLCanvasElement;
  status: HTMLElement;
  opponentNavigation: HTMLDivElement;
  returnBoard: HTMLButtonElement;
  stackPanel: HTMLDivElement;
  stackCount: HTMLOutputElement;
  resolvePreview?: HTMLButtonElement;
  readyMessage?: string;
  playerNames?: readonly string[];
  playerPortraits?: readonly THREE.Texture[];
  playerHandCounts?: readonly number[];
  playerZones?: readonly PublicPlayerZones[];
  title?: HTMLElement;
  hint?: HTMLElement;
  sleeveAttribution?: HTMLElement;
  onFrame?: () => void;
};
export type CardSceneHandle = { dispose(): void; readonly disposed: boolean };
export type CardSceneOptions = {
  assetBaseUrl?: string | URL;
  signal?: AbortSignal;
  /** Preview behavior is opt-in. Integration never treats a gesture as an accepted play. */
  playMode?: "fixture-preview" | "integration";
  onPlayIntent?: (
    intent: TableDropIntent,
    signal: AbortSignal,
  ) => void | Promise<void>;
};
/** Rounded silhouette in the XY plane at Magic card proportions. */
function silhouette(w = 2.5, h = 3.49, r = 0.12): THREE.Shape {
  const x = -w / 2,
    y = -h / 2,
    s = new THREE.Shape();
  s.moveTo(x + r, y);
  s.lineTo(x + w - r, y);
  s.quadraticCurveTo(x + w, y, x + w, y + r);
  s.lineTo(x + w, y + h - r);
  s.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  s.lineTo(x + r, y + h);
  s.quadraticCurveTo(x, y + h, x, y + h - r);
  s.lineTo(x, y + r);
  s.quadraticCurveTo(x, y, x + r, y);
  return s;
}
/** Map a rounded face to a full image without stretching its margins. */
function mappedFace(
  shape: THREE.Shape,
  width: number,
  height: number,
): THREE.ShapeGeometry {
  const geometry = new THREE.ShapeGeometry(shape, 16);
  const positions = geometry.getAttribute("position"),
    uv = geometry.getAttribute("uv");
  for (let i = 0; i < positions.count; i++)
    uv.setXY(
      i,
      positions.getX(i) / width + 0.5,
      positions.getY(i) / height + 0.5,
    );
  return geometry;
}
/** Shared card/table renderer. Page shells own their setup and visible controls. */
export async function mountCardScene(
  surface: CardSceneSurface,
  settings: SceneSettings,
  options: CardSceneOptions = {},
): Promise<CardSceneHandle> {
  const lifetime = createSceneLifecycle(options.signal);
  lifetime.assertActive();
  lifetime.add(() => {
    surface.opponentNavigation.replaceChildren();
    surface.opponentNavigation.hidden = true;
    surface.returnBoard.hidden = true;
    surface.stackPanel.hidden = true;
  });
  try {
    const assetBase = new URL(
      String(options.assetBaseUrl ?? "/"),
      surface.canvas.ownerDocument.baseURI,
    );
    const assetUrl = (path: string): string => sceneAssetUrl(path, assetBase);
    const {
      canvas,
      status,
      opponentNavigation,
      returnBoard,
      stackPanel,
      stackCount,
      resolvePreview,
    } = surface;
    const button = (id: SceneSettingId) => settings[id];
    const tilt = settings["tilt"];
    const turn = settings["turn"];
    const light = settings["light"];
    const tableZoom = settings["table-zoom"];
    const view = settings["view"];
    const cardBackChoice = settings["card-back"];
    const sleeveChoice = settings["sleeve-design"];
    const ordinary = settings["ordinary"];
    const foil = settings["foil"];
    const flip = settings["flip"];
    const sleeveToggle = settings["sleeve"];
    const deckCount = settings["deck-count"];
    const deckMode = settings["deck-mode"];
    const comparisonOutput = settings["deck-performance"];
    const deckCountLabel = settings["deck-count-label"];
    const playerCount = settings["player-count"];
    const playerFocus = settings["player-focus"];
    const navigationMode = settings["navigation-mode"];
    let attackRecipient: number | null = null;
    const viewerSeat = settings["viewer-seat"];
    const tableMode = settings["table-mode"];
    const handSize = settings["hand-size"];
    const opponentHandSize = settings["opponent-hand-size"];
    const handVisibility = settings["show-hands"];
    const handSummary = settings["hand-summary"];
    const inspectHand = settings["inspect-hand"];
    const interactionStatus = settings["interaction-status"];
    const autoTap = settings["auto-tap"];
    const neighbors = settings["seat-neighbors"];
    const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
    const state = { foil: false, back: false, sleeve: false };
    autoTap.addEventListener(
      "change",
      () => {
        interactionStatus.value = autoTap.checked
          ? "Auto-tap preference enabled"
          : "Manual mana preference enabled";
      },
      { signal: lifetime.signal },
    );
    canvas.addEventListener(
      "webglcontextlost",
      (event) => {
        event.preventDefault();
        status.textContent = "Graphics context lost. Reload to retry.";
      },
      { signal: lifetime.signal },
    );

    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    lifetime.add(() => renderer.dispose());
    renderer.setPixelRatio(Math.min(2, Math.max(devicePixelRatio, 1.5)));
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.0;
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    const scene = new THREE.Scene();
    const textures = new Set<THREE.Texture>();
    const geometries = new Set<THREE.BufferGeometry>();
    const materials = new Set<THREE.Material>();
    const instances = new Set<THREE.InstancedMesh>();
    function own<T extends THREE.Material | THREE.BufferGeometry>(
      resource: T,
    ): T {
      if (resource instanceof THREE.Material) materials.add(resource);
      else geometries.add(resource);
      return resource;
    }
    function collect(root: THREE.Object3D): void {
      root.traverse((part) => {
        if (part instanceof THREE.InstancedMesh) instances.add(part);
        if (part.userData.commanderAura === true) return;
        if (
          part instanceof THREE.Mesh ||
          part instanceof THREE.Line ||
          part instanceof THREE.Points
        ) {
          geometries.add(part.geometry);
          for (const material of Array.isArray(part.material)
            ? part.material
            : [part.material])
            materials.add(material);
        }
      });
    }
    lifetime.add(() => {
      collect(scene);
      for (const instance of instances) instance.dispose();
      for (const geometry of geometries) geometry.dispose();
      for (const material of materials) material.dispose();
      for (const texture of textures) texture.dispose();
      scene.clear();
    });
    scene.background = new THREE.Color(0x202322);
    const fog = new THREE.Fog(0x202322, 8, 24);
    scene.fog = fog;
    const environment = new RoomEnvironment(),
      pmrem = new THREE.PMREMGenerator(renderer);
    const environmentTarget = pmrem.fromScene(environment, 0.04);
    lifetime.add(() => environmentTarget.dispose());
    scene.environment = environmentTarget.texture;
    scene.environmentIntensity = 0.65;
    environment.dispose();
    pmrem.dispose();
    const camera = new THREE.PerspectiveCamera(36, 1, 0.1, 60);
    scene.add(new THREE.HemisphereLight(0xfff7e7, 0x424e49, 1.05));
    const key = new THREE.DirectionalLight(0xffedce, 1.9);
    key.position.set(-3, 5, 5);
    key.castShadow = true;
    key.shadow.mapSize.set(2048, 2048);
    key.shadow.camera.left = -5;
    key.shadow.camera.right = 5;
    key.shadow.camera.top = 5;
    key.shadow.camera.bottom = -5;
    key.shadow.normalBias = 0.008;
    key.shadow.bias = -0.0001;
    key.shadow.radius = 3;
    lifetime.add(() => key.shadow.dispose());
    scene.add(key);
    const fill = new THREE.DirectionalLight(0xc2daef, 0.7);
    fill.position.set(4, 2, -3);
    scene.add(fill);
    const loader = new THREE.TextureLoader();
    async function loadTexture(path: string): Promise<THREE.Texture> {
      const texture = await loader.loadAsync(assetUrl(path));
      if (lifetime.disposed) texture.dispose();
      else textures.add(texture);
      lifetime.assertActive();
      return texture;
    }
    const [front, back, mountain] = await Promise.all([
      loadTexture("cards/lightning-bolt.png"),
      loadTexture("cards/magic-back.jpg"),
      loadTexture("cards/mountain-m11.png"),
    ]);
    for (const texture of [front, back, mountain]) {
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
    }
    const shape = silhouette();
    const faceGeometry = own(mappedFace(shape, 2.5, 3.49));
    geometries.add(faceGeometry);
    const stockGeometry = own(
      new THREE.ExtrudeGeometry(shape, {
        depth: 0.012,
        bevelEnabled: true,
        bevelSize: 0.002,
        bevelThickness: 0.002,
        bevelSegments: 2,
        steps: 1,
        curveSegments: 16,
      }),
    );
    stockGeometry.translate(0, 0, -0.006);
    stockGeometry.scale(1, 1, CARD_THICKNESS_SCALE);
    const card = new THREE.Group(),
      stock = new THREE.Mesh(
        stockGeometry,
        own(
          new THREE.MeshStandardMaterial({ color: 0x8c8a7d, roughness: 0.93 }),
        ),
      );
    stock.castShadow = true;
    card.add(stock);
    const finishUniform = { value: 0 };
    const facingFade = { value: 1 };
    const frontMaterial = own(
      new THREE.MeshPhysicalMaterial({
        map: front,
        roughness: 0.76,
        metalness: 0,
        specularIntensity: 0.18,
        clearcoat: 0.035,
        clearcoatRoughness: 0.6,
        envMapIntensity: 0.16,
      }),
    );
    // Approximate diffraction in the opaque face pass, driven by view and light angle.
    const foilLight = { value: new THREE.Vector3(-0.6, 0.8, 1).normalize() };
    frontMaterial.onBeforeCompile = (shader): void => {
      shader.uniforms.foilAmount = finishUniform;
      shader.uniforms.cardFacingFade = facingFade;
      shader.uniforms.foilLight = foilLight;
      shader.fragmentShader =
        "uniform float foilAmount; uniform vec3 foilLight; uniform float cardFacingFade;\n" +
        shader.fragmentShader;
      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <color_fragment>",
        `#include <color_fragment>
 vec3 viewDirection=normalize(vViewPosition);vec3 surfaceNormal=normalize(vNormal);
 float facing=dot(surfaceNormal,viewDirection);
 float phase=dot(surfaceNormal,foilLight)*2.4+(1.0-facing)*3.0+vMapUv.x*1.05+vMapUv.y*0.72;
 vec3 spectrum=0.5+0.5*cos(6.2831853*(phase+vec3(0.0,0.333,0.667)));
 float reflection=pow(max(0.0,dot(reflect(-foilLight,surfaceNormal),viewDirection)),12.0);
 // This study's M11 artwork window receives the strongest treatment.
 // Soften the transition into the title and rules areas to preserve legibility.
 float artMask=smoothstep(0.30,0.38,vMapUv.y)*(1.0-smoothstep(0.78,0.85,vMapUv.y));
 float strength=0.28+0.72*artMask;
 float ribbon=pow(0.5+0.5*sin(phase*6.2831853),6.0);
 vec3 foilColor=diffuseColor.rgb*(0.64+spectrum*0.90)+spectrum*(reflection*0.16+ribbon*0.055);
 diffuseColor.rgb=mix(diffuseColor.rgb,foilColor,foilAmount*strength);
 diffuseColor.rgb=mix(vec3(0.025,0.028,0.026),diffuseColor.rgb,cardFacingFade);
 `,
      );
    };
    frontMaterial.customProgramCacheKey = (): string =>
      "spellwake-foil-facing-v3";
    const face = new THREE.Mesh(faceGeometry, frontMaterial);
    face.position.z = 0.0092 * CARD_THICKNESS_SCALE;
    face.userData.printedFront = true;
    card.add(face);
    const reverse = new THREE.Mesh(
      faceGeometry,
      own(
        new THREE.MeshPhysicalMaterial({
          map: back,
          roughness: 0.6,
          clearcoat: 0.12,
          clearcoatRoughness: 0.4,
          envMapIntensity: 0.3,
        }),
      ),
    );
    reverse.rotation.y = Math.PI;
    reverse.position.z = -0.0092 * CARD_THICKNESS_SCALE;
    card.add(reverse);
    // Sleeve dimensions include a small margin and lip around standard cardstock.
    const sleeve = new THREE.Group();
    const sleeveShape = silhouette(2.62, 3.65, 0.11);
    const sleeveFaceGeometry = own(mappedFace(sleeveShape, 2.62, 3.65));
    const [midnightArt, dragonArt, unletteredBackArt] = await Promise.all([
      loadTexture("cards/spellwake-sleeve-midnight.svg"),
      loadTexture("cards/sleeve-shivan-dragon.jpg"),
      loadTexture("cards/magic-unlettered.png"),
    ]);
    for (const texture of [dragonArt, unletteredBackArt]) {
      texture.colorSpace = THREE.SRGBColorSpace;
      texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
    }
    const dragonImage = z.instanceof(HTMLImageElement).parse(dragonArt.image);
    const cropWidth = Math.min(
      1,
      2.62 / 3.65 / (dragonImage.width / dragonImage.height),
    );
    dragonArt.repeat.x = cropWidth;
    dragonArt.offset.x = (1 - cropWidth) / 2;
    const supporterNames = [
      "initiate",
      "adept",
      "mage",
      "archmage",
      "benefactor",
    ] as const;
    const supporterSchema = z.enum(supporterNames);
    const supporterArt = new Map<string, THREE.Texture>();
    const supporterMasks = new Map<string, THREE.Texture>();
    await Promise.all(
      supporterNames.map(async (name) => {
        const texture = await loadTexture(`cards/supporter-${name}.svg`);
        texture.colorSpace = THREE.SRGBColorSpace;
        texture.anisotropy = Math.min(
          8,
          renderer.capabilities.getMaxAnisotropy(),
        );
        supporterArt.set(name, texture);
        {
          const mask = await loadTexture(`cards/supporter-${name}-mask.svg`);
          mask.colorSpace = THREE.NoColorSpace;
          mask.anisotropy = texture.anisotropy;
          supporterMasks.set(name, mask);
        }
      }),
    );
    /** Selective metallic ink, independent from the original card-face foil treatment. */
    function ornamentalFoil(
      material: THREE.MeshPhysicalMaterial,
      cacheKey: string,
    ) {
      const amount = { value: 0 },
        mask = { value: supporterMasks.get("benefactor") };
      const previousCompile = material.onBeforeCompile.bind(material);
      material.onBeforeCompile = (shader): void => {
        previousCompile(shader, renderer);
        shader.uniforms.ornamentAmount = amount;
        shader.uniforms.ornamentMask = mask;
        shader.uniforms.ornamentLight = foilLight;
        shader.fragmentShader =
          "uniform float ornamentAmount; uniform sampler2D ornamentMask; uniform vec3 ornamentLight;\n" +
          shader.fragmentShader;
        shader.fragmentShader = shader.fragmentShader.replace(
          "#include <color_fragment>",
          `#include <color_fragment>
#ifdef USE_MAP
 float ink = texture2D(ornamentMask, vMapUv).r;
 float phase = dot(normalize(vNormal), ornamentLight)*1.8 + dot(normalize(vNormal),normalize(vViewPosition))*2.0 + vMapUv.x*0.6 + vMapUv.y*0.35;
 vec3 spectrum = 0.5+0.5*cos(6.2831853*(phase+vec3(0.0,0.333,0.667)));
 vec3 metallicInk = diffuseColor.rgb*(0.75+spectrum*0.65)+spectrum*0.12;
 diffuseColor.rgb=mix(diffuseColor.rgb,metallicInk,ornamentAmount*ink);
#endif
`,
        );
        shader.fragmentShader = shader.fragmentShader
          .replace(
            "#include <roughnessmap_fragment>",
            `#include <roughnessmap_fragment>
#ifdef USE_MAP
 roughnessFactor = mix(roughnessFactor, 0.24, ornamentAmount * ink);
#endif`,
          )
          .replace(
            "#include <metalnessmap_fragment>",
            `#include <metalnessmap_fragment>
#ifdef USE_MAP
 metalnessFactor = mix(metalnessFactor, 0.82, ornamentAmount * ink);
#endif`,
          );
        // A directional diffraction lobe supplements the small studio environment.
        // Only the stamped ink receives it; the printed field stays nonmetallic.
        shader.fragmentShader = shader.fragmentShader.replace(
          "#include <opaque_fragment>",
          `#ifdef USE_MAP
 float stampGlint = pow(0.5+0.5*sin(phase*6.2831853 + vMapUv.y*4.0), 10.0);
 outgoingLight += ornamentAmount * ink * (diffuseColor.rgb*0.32 + (vec3(0.65)+spectrum*0.65)*stampGlint*1.65);
#endif
#include <opaque_fragment>`,
        );
      };
      material.customProgramCacheKey = (): string =>
        `spellwake-ornamental-foil-v3-${cacheKey}`;
      return { amount, mask };
    }
    const reverseFoil = ornamentalFoil(reverse.material, "card-back");
    const sleeveDesignSchema = z.enum([
      ...supporterNames,
      "midnight",
      "black-matte",
      "black-gloss",
      "dragon-gloss",
      "penny",
      "rigid",
    ]);
    // Reference-derived grain affects lighting only; pigment and artwork remain independent.
    const [matteNormal, matteRoughness] = await Promise.all([
      loadTexture("cards/matte-plastic-normal.png"),
      loadTexture("cards/matte-plastic-roughness.png"),
    ]);
    for (const texture of [matteNormal, matteRoughness]) {
      texture.colorSpace = THREE.NoColorSpace;
      texture.wrapS = texture.wrapT = THREE.ClampToEdgeWrapping;
      texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
    }
    midnightArt.colorSpace = THREE.SRGBColorSpace;
    midnightArt.anisotropy = Math.min(
      8,
      renderer.capabilities.getMaxAnisotropy(),
    );
    const sleeveStockGeometry = own(
      new THREE.ExtrudeGeometry(sleeveShape, {
        depth: 0.008,
        bevelEnabled: false,
        curveSegments: 16,
        steps: 1,
      }),
    );
    sleeveStockGeometry.translate(0, 0, -0.026);
    const sleeveStock = new THREE.Mesh(
      sleeveStockGeometry,
      own(
        new THREE.MeshStandardMaterial({
          color: 0x182233,
          roughness: 0.66,
        }),
      ),
    );
    sleeveStock.castShadow = true;
    sleeve.add(sleeveStock);
    const sleeveBack = new THREE.Mesh(
      sleeveFaceGeometry,
      own(
        new THREE.MeshPhysicalMaterial({
          map: midnightArt,
          roughness: 0.59,
          clearcoat: 0.12,
          clearcoatRoughness: 0.48,
          specularIntensity: 0.3,
          envMapIntensity: 0.25,
        }),
      ),
    );
    const matteBackUniform = { value: 0 };
    sleeveBack.material.onBeforeCompile = (shader): void => {
      shader.uniforms.matteBack = matteBackUniform;
      shader.fragmentShader =
        "uniform float matteBack;\n" + shader.fragmentShader;
      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <roughnessmap_fragment>",
        "#include <roughnessmap_fragment>\nroughnessFactor = mix(roughnessFactor, 0.48 + roughnessFactor * 0.4, matteBack);",
      );
    };
    const sleeveFoil = ornamentalFoil(sleeveBack.material, "sleeve-back");
    sleeveBack.rotation.y = Math.PI;
    sleeveBack.position.z = -0.0265;
    sleeve.add(sleeveBack);
    function sealingRim(
      innerWidth: number,
      innerHeight: number,
    ): THREE.ShapeGeometry {
      const rimShape = silhouette(2.62, 3.65, 0.11);
      rimShape.holes.push(
        new THREE.Path(
          silhouette(innerWidth, innerHeight, 0.105).getPoints(16),
        ),
      );
      const rimGeometry = own(new THREE.ShapeGeometry(rimShape, 16));
      const rimPositions = rimGeometry.getAttribute("position"),
        rimIndices = rimGeometry.getIndex();
      if (rimIndices) {
        const openIndices: number[] = [];
        for (let i = 0; i < rimIndices.count; i += 3) {
          const a = rimIndices.getX(i),
            b = rimIndices.getX(i + 1),
            c = rimIndices.getX(i + 2);
          if (
            (rimPositions.getY(a) +
              rimPositions.getY(b) +
              rimPositions.getY(c)) /
              3 <
            1.715
          )
            openIndices.push(a, b, c);
        }
        rimGeometry.setIndex(openIndices);
      }
      return rimGeometry;
    }
    const rigidRimGeometry = sealingRim(2.358, 3.376);
    const rim = new THREE.Mesh(
      rigidRimGeometry,
      own(
        new THREE.MeshPhysicalMaterial({
          color: 0x283449,
          roughness: 0.43,
          specularIntensity: 0.45,
          side: THREE.DoubleSide,
        }),
      ),
    );
    rim.position.z = 0.023;
    sleeve.add(rim);
    // Join the two panels around the outer contour, omitting only the top opening.
    const contour = sleeveShape.getPoints(24),
      seamVertices: number[] = [];
    for (let i = 0; i < contour.length - 1; i++) {
      const a = contour[i],
        b = contour[i + 1];
      if (!a || !b || (a.y >= 1.8249 && b.y >= 1.8249)) continue;
      seamVertices.push(
        a.x,
        a.y,
        -0.026,
        b.x,
        b.y,
        -0.026,
        b.x,
        b.y,
        0.024,
        a.x,
        a.y,
        -0.026,
        b.x,
        b.y,
        0.024,
        a.x,
        a.y,
        0.024,
      );
    }
    const seamGeometry = own(new THREE.BufferGeometry());
    seamGeometry.setAttribute(
      "position",
      new THREE.Float32BufferAttribute(seamVertices, 3),
    );
    seamGeometry.computeVertexNormals();
    const seams = new THREE.Mesh(
      seamGeometry,
      own(
        new THREE.MeshPhysicalMaterial({
          color: 0x283449,
          roughness: 0.48,
          specularIntensity: 0.35,
          side: THREE.DoubleSide,
        }),
      ),
    );
    seams.castShadow = true;
    sleeve.add(seams);
    // Low-opacity coating gives the clear film a highlight without washing out the art.
    const sleeveFront = new THREE.Mesh(
      sleeveFaceGeometry,
      own(
        new THREE.MeshPhysicalMaterial({
          color: 0xf4fff9,
          transparent: true,
          opacity: 0.035,
          depthWrite: false,
          roughness: 0.32,
          metalness: 0,
          clearcoat: 0.4,
          clearcoatRoughness: 0.25,
          specularIntensity: 0.4,
          envMapIntensity: 0.35,
        }),
      ),
    );
    sleeveFront.position.z = 0.024;
    sleeveFront.renderOrder = 1;
    sleeve.add(sleeveFront);
    // Thin film panels taper into one welded edge; the rigid holder retains its walls.
    const rigidStockGeometry = sleeveStock.geometry;
    const filmFaceGeometry = own(sleeveFaceGeometry.clone());
    const panelScaleX = 2.555 / 2.62,
      panelScaleY = (3.565 + 3.65) / 2 / 3.65,
      panelOffsetY = (3.65 - 3.565) / 4;
    filmFaceGeometry.scale(panelScaleX, panelScaleY, 1);
    filmFaceGeometry.translate(0, panelOffsetY, 0);
    const filmStockGeometry = own(rigidStockGeometry.clone());
    filmStockGeometry.scale(panelScaleX, panelScaleY, 0.35);
    filmStockGeometry.translate(0, panelOffsetY, -0.004);
    const filmSeamGeometry = own(
      createPressedSleeveSeam(contour, {
        panelScaleX,
        panelScaleY,
        panelOffsetY,
        backZ: -0.0136,
        frontZ: 0.012,
        sealThickness: 0.002,
        openingY: 1.8249,
      }),
    );
    function setSleeveProfile(isRigid: boolean): void {
      sleeveStock.geometry = isRigid ? rigidStockGeometry : filmStockGeometry;
      sleeveBack.geometry = sleeveFront.geometry = isRigid
        ? sleeveFaceGeometry
        : filmFaceGeometry;
      sleeveBack.position.z = isRigid ? -0.0265 : -0.0136;
      sleeveFront.position.z = isRigid ? 0.024 : 0.012;
      seams.geometry = isRigid ? seamGeometry : filmSeamGeometry;
      rim.visible = isRigid;
    }
    setSleeveProfile(false);

    sleeve.visible = state.sleeve;
    if (state.sleeve) card.add(sleeve);
    scene.add(card);
    const floor = new THREE.Mesh(
      own(new THREE.PlaneGeometry(100, 100)),
      own(new THREE.MeshStandardMaterial({ color: 0x303832, roughness: 0.95 })),
    );
    floor.rotation.x = -Math.PI / 2;
    floor.position.y = -1.92;
    floor.receiveShadow = true;
    scene.add(floor);
    const gameTable = new THREE.Group();
    scene.add(gameTable);
    const playmat = new THREE.Mesh(
      own(new THREE.BoxGeometry(16, 0.1, 12)),
      own(new THREE.MeshStandardMaterial({ color: 0x182a29, roughness: 0.96 })),
    );
    playmat.position.y = floor.position.y + 0.05;
    playmat.receiveShadow = true;
    gameTable.add(playmat);
    const laidOutCards = new THREE.Group();
    gameTable.add(laidOutCards);
    // Simple instanced edge plates preserve each layer's silhouette without its art/material passes.
    const edgeGeometry = own(
      new THREE.ExtrudeGeometry(silhouette(), {
        depth: 1,
        bevelEnabled: false,
        curveSegments: 3,
      }),
    );
    edgeGeometry.translate(0, 0, -0.5);
    const edgeMaterial = own(
      new THREE.MeshStandardMaterial({
        color: 0xffffff,
        roughness: 0.84,
      }),
    );
    let edgeStack: THREE.InstancedMesh | null = null;
    const mountainMaterial = own(frontMaterial.clone());
    materials.add(mountainMaterial);
    mountainMaterial.map = mountain;
    mountainMaterial.onBeforeCompile = frontMaterial.onBeforeCompile;
    mountainMaterial.customProgramCacheKey = (): string =>
      "spellwake-mountain-foil-study";
    frontMaterial.name = "Lightning Bolt";
    mountainMaterial.name = "Mountain";
    const boardMaterials = new Map<string, THREE.MeshPhysicalMaterial>();
    await Promise.all(
      BOARD_SAMPLES.map(async ([key, name]) => {
        const texture = await loadTexture(`cards/${key}.png`);
        texture.colorSpace = THREE.SRGBColorSpace;
        texture.anisotropy = renderer.capabilities.getMaxAnisotropy();
        const material = own(frontMaterial.clone());
        material.map = texture;
        material.name = name;
        material.onBeforeCompile = frontMaterial.onBeforeCompile;
        material.customProgramCacheKey = () => `spellwake-board-${key}`;
        materials.add(material);
        boardMaterials.set(key, material);
      }),
    );
    boardMaterials.set("lightning-bolt", frontMaterial);
    boardMaterials.set("land", mountainMaterial);
    const layout = createTableLayout(
      floor.position.y + 0.11,
      mountainMaterial,
      boardMaterials,
    );
    scene.add(layout.group);
    scene.add(camera);
    const hands = createHandStudy(
      camera,
      mountainMaterial,
      boardMaterials.get("krenko"),
      boardMaterials,
      {
        ...(canvas.parentElement ? { host: canvas.parentElement } : {}),
        changed: requestDraw,
        fixture: options.playMode === "fixture-preview",
        read: (card) => {
          inspection.open(card);
          requestDraw();
        },
      },
    );
    const inspection = createCardInspection(camera);
    scene.add(hands.opponents, hands.opponentExternal);
    const plays = createPlayPreview(scene, camera);
    lifetime.add(() => {
      collect(card);
      collect(sleeve);
      collect(laidOutCards);
    });
    lifetime.add(() => hands.dispose());
    const interaction = createTableInteraction(
      options.playMode === "fixture-preview"
        ? {
            mode: "fixture-preview",
            onPreview: (intent) => {
              const source = scene.getObjectByProperty(
                "uuid",
                intent.presentationCardId,
              );
              const board = layout.group.children[Number(viewerSeat.value) - 1];
              if (source && board)
                plays.drop(source, board, floor.position.y + 0.11);
            },
          }
        : {
            mode: "integration",
            ...(options.onPlayIntent ? { onIntent: options.onPlayIntent } : {}),
          },
    );
    lifetime.add(() => interaction.cancel());
    const piles = createPileBrowser(
      camera,
      inspection,
      () =>
        laidOutCards.children.find(
          (child) =>
            child.userData.isStudyCard === true &&
            !(child instanceof THREE.InstancedMesh),
        ),
      boardMaterials,
      requestDraw,
      canvas.parentElement ?? undefined,
    );
    lifetime.add(() => piles.dispose());
    resolvePreview?.addEventListener(
      "click",
      () => {
        if (options.playMode === "fixture-preview" && plays.resolveTop()) {
          interactionStatus.value =
            "Resolution preview · Instant moves to graveyard";
          requestDraw();
        }
      },
      { signal: lifetime.signal },
    );
    let layoutSignature = "";
    const layoutCenter = new THREE.Vector3();
    const layoutSize = new THREE.Vector3();
    function updateLayout(): void {
      const signature = `${tableSignature}:${playerCount.value}:${viewerSeat.value}:${tableMode.value}:${handSize.value}:${opponentHandSize.value}`;
      if (signature === layoutSignature) return;
      inspection.cancel();
      piles.close();
      plays.clear();
      layoutSignature = signature;
      const count = Number(playerCount.value);
      const viewer = Math.min(Number(viewerSeat.value), count);
      viewerSeat.value = String(viewer);
      const mode = z
        .enum(["ffa", "one-two", "two-one", "two-two"])
        .parse(tableMode.value);
      layout.rebuild(laidOutCards, count, viewer, mode);
      const handTemplate = laidOutCards.children.find(
        (child) => child.userData.isStudyCard === true,
      );
      if (handTemplate)
        hands.rebuild(
          handTemplate,
          layout.group,
          viewer,
          Number(handSize.value),
          Number(opponentHandSize.value),
          floor.position.y + 0.11,
          surface.playerHandCounts,
          surface.playerZones,
        );
      handSummary.value = `Your hand: ${handSize.value} · Each opponent: ${opponentHandSize.value}. Sample cards only.`;
      inspectHand.disabled = Number(handSize.value) === 0;
      neighbors.value = `P${viewer} · Left: P${(viewer % count) + 1} · Right: P${((viewer + count - 2) % count) + 1}. Clockwise: ${Array.from({ length: count }, (_, i) => "P" + (i + 1)).join(" → ")} → P1`;
      for (const option of Array.from(viewerSeat.options))
        option.disabled = Number(option.value) > count;
      for (const option of Array.from(playerFocus.options)) {
        const id = Number(option.value);
        option.textContent =
          id === 0
            ? "Whole table"
            : id === viewer
              ? `Your battlefield · P${id}`
              : `Player ${id}`;
      }
      if (Number(playerFocus.value) > Number(playerCount.value))
        playerFocus.value = viewerSeat.value;
      for (const option of Array.from(playerFocus.options))
        option.disabled = Number(option.value) > Number(playerCount.value);
    }

    const tableCamera = createTableCameraMotion();
    const cardFacing = createCardFacingFade();
    const interfaceFont = new FontFace(
      "Albert Sans",
      `url(${JSON.stringify(assetUrl("fonts/albertsans.ttf"))})`,
    );
    await interfaceFont.load();
    lifetime.assertActive();
    document.fonts.add(interfaceFont);
    lifetime.add(() => {
      document.fonts.delete(interfaceFont);
    });
    // Card-art crops are fixture portraits; account textures can replace them at the surface boundary.
    const portraits =
      surface.playerPortraits ??
      [
        "serra-angel",
        "chandra-nalaar",
        "krenko",
        "llanowar-elves",
        "runeclaw-bear",
        "pacifism",
      ].map((key) => {
        const avatar = document.createElement("canvas");
        avatar.width = avatar.height = 128;
        const context = avatar.getContext("2d");
        const source = boardMaterials.get(key)?.map?.image;
        if (context && source instanceof HTMLImageElement)
          context.drawImage(
            source,
            source.width * 0.18,
            source.height * 0.14,
            source.width * 0.64,
            source.width * 0.64,
            0,
            0,
            128,
            128,
          );
        const texture = new THREE.CanvasTexture(avatar);
        textures.add(texture);
        texture.colorSpace = THREE.SRGBColorSpace;
        return texture;
      });
    const tableNavigation = createTableNavigation(
      floor.position.y + 0.11,
      portraits,
    );
    scene.add(tableNavigation.group);
    lifetime.add(() => tableNavigation.dispose());
    opponentNavigation.classList.add("sr-only");
    returnBoard.classList.add("sr-only");
    let navigationSignature = "";
    function focusBoard(seat: number): void {
      inspection.cancel();
      piles.close();
      hands.hover(-1);
      playerFocus.value = String(seat);
      requestDraw();
    }
    const overview = canvas.parentElement
      ? createTableOverview(
          canvas.parentElement,
          focusBoard,
          surface.playerNames,
          portraits.map((portrait) => {
            const image: unknown = portrait.image;
            return image instanceof HTMLImageElement ||
              image instanceof HTMLCanvasElement
              ? image
              : null;
          }),
        )
      : null;
    lifetime.add(() => overview?.dispose());
    returnBoard.addEventListener(
      "click",
      () => focusBoard(Number(viewerSeat.value)),
      { signal: lifetime.signal },
    );
    function updateNavigation(): void {
      const focus = Number(playerFocus.value) || Number(viewerSeat.value);
      const isAttack = navigationMode.value === "attack";
      const signature = `${layoutSignature}:${focus}:${isAttack}:${attackRecipient}`;
      if (signature === navigationSignature) return;
      navigationSignature = signature;
      opponentNavigation.replaceChildren();
      const origin = layout.seats[focus - 1];
      if (!origin) return;
      const yaw = layout.perspective(focus);
      const targets = navigationDirections(layout.seats, focus, yaw);
      for (const target of targets) {
        const control = document.createElement("button");
        control.type = "button";
        const name =
          target.seat === Number(viewerSeat.value)
            ? "your board"
            : `${surface.playerNames?.[target.seat - 1] ?? `Player ${target.seat}`}'s board`;
        const label = isAttack
          ? `Choose ${surface.playerNames?.[target.seat - 1] ?? `Player ${target.seat}`} as attack recipient`
          : `View ${name}`;
        control.setAttribute("aria-label", label);
        control.title = label;
        if (isAttack)
          control.setAttribute(
            "aria-pressed",
            String(attackRecipient === target.seat),
          );
        control.addEventListener(
          "click",
          () => activatePlayerControl(target.seat),
          { signal: lifetime.signal },
        );
        opponentNavigation.append(control);
      }
      tableNavigation.rebuild(
        origin,
        yaw,
        targets,
        Number(viewerSeat.value),
        focus !== Number(viewerSeat.value),
      );
      tableNavigation.setAttackMode(isAttack, attackRecipient);
    }
    function activatePlayerControl(seat: number): void {
      if (navigationMode.value !== "attack") {
        focusBoard(seat);
        return;
      }
      attackRecipient = seat;
      status.textContent = `Attack recipient preview: ${surface.playerNames?.[seat - 1] ?? `Player ${seat}`}`;
      requestDraw();
    }

    let tableSignature = "";
    /** Reuse the approved geometry/materials; clones own only their transforms. */
    function updateTable(): void {
      const signature = `${state.sleeve}:${sleeveChoice.value}:${state.back}:${deckMode.value}:${deckCount.value}`;
      if (signature === tableSignature) return;
      inspection.cancel();
      tableSignature = signature;
      edgeStack?.dispose();
      edgeStack = null;
      laidOutCards.clear();
      const surface = floor.position.y + 0.11;
      const bounds = new THREE.Box3();
      const addCard = (
        x: number,
        z: number,
        angle: number,
        faceDown: boolean,
        lift = 0,
      ): void => {
        const instance = card.clone();
        instance.visible = true;
        instance.userData.faceDown = faceDown;
        instance.userData.isStudyCard = true;
        instance.position.set(x, 0, z);
        instance.rotation.set(
          faceDown ? Math.PI / 2 : -Math.PI / 2,
          0,
          angle + (faceDown ? Math.PI : 0),
        );
        bounds.setFromObject(instance);
        instance.position.y = surface + 0.006 - bounds.min.y + lift;
        laidOutCards.add(instance);
      };
      addCard(-4.8, 2, -0.06, state.back);
      addCard(-1.55, 2, 0.025, state.back);
      addCard(1.7, 2, 0.08, state.back);
      addCard(-4.3, -2.7, -0.12, true);
      addCard(-0.95, -2.6, 0.06, true);
      const spacing = state.sleeve
        ? sleeveChoice.value === "rigid"
          ? 0.18
          : 0.032 * CARD_THICKNESS_SCALE
        : 0.022 * CARD_THICKNESS_SCALE;
      const count = z.coerce
        .number()
        .int()
        .min(0)
        .max(200)
        .parse(deckCount.value);
      deckCountLabel.value = `${count} ${count === 1 ? "card" : "cards"}`;
      if (deckMode.value === "optimized" && count > 0) {
        const detailedCount = Math.min(4, count);
        const edgeCount = count - detailedCount;
        const sizeX = state.sleeve ? (2.62 * sleeve.scale.x) / 2.5 : 1;
        const sizeY = state.sleeve ? (3.65 * sleeve.scale.y) / 3.49 : 1;
        if (edgeCount > 0) {
          edgeStack = new THREE.InstancedMesh(
            edgeGeometry,
            edgeMaterial,
            edgeCount,
          );
          edgeStack.castShadow = true;
          edgeStack.userData.faceDown = true;
          edgeStack.userData.isStudyCard = true;
          const transform = new THREE.Object3D();
          const color = new THREE.Color();
          const baseColor =
            state.sleeve && sleeveStock.visible
              ? sleeveStock.material.color
              : stock.material.color;
          for (let i = 0; i < edgeCount; i++) {
            transform.position.set(
              5.05 + Math.sin(i * 2.4) * 0.018,
              surface + 0.006 + i * spacing + spacing * 0.45,
              -1.9 + Math.cos(i * 1.7) * 0.014,
            );
            transform.rotation.set(Math.PI / 2, 0, Math.sin(i * 0.8) * 0.004);
            transform.scale.set(
              sizeX,
              sizeY,
              spacing * (0.88 + Math.sin(i * 1.3) * 0.025),
            );
            transform.updateMatrix();
            edgeStack.setMatrixAt(i, transform.matrix);
            color
              .copy(baseColor)
              .multiplyScalar(0.94 + (0.5 + 0.5 * Math.sin(i * 2.1)) * 0.12);
            edgeStack.setColorAt(i, color);
          }
          edgeStack.instanceMatrix.needsUpdate = true;
          if (edgeStack.instanceColor)
            edgeStack.instanceColor.needsUpdate = true;
          laidOutCards.add(edgeStack);
        }
        for (let i = edgeCount; i < count; i++) {
          addCard(
            5.05 + Math.sin(i * 2.4) * 0.018,
            -1.9 + Math.cos(i * 1.7) * 0.014,
            Math.sin(i * 0.8) * 0.004,
            true,
            i * spacing,
          );
        }
        return;
      }
      for (let i = 0; i < count; i++) {
        const offset = Math.sin(i * 2.4) * 0.018;
        addCard(
          5.05 + offset,
          -1.9 + Math.cos(i * 1.7) * 0.014,
          Math.sin(i * 0.8) * 0.004,
          true,
          i * spacing,
        );
      }
    }
    let targetFlip = 0,
      currentFlip = 0,
      previous = performance.now(),
      pending = false;
    const cardBounds = new THREE.Box3();
    const rotation = new THREE.Vector3(
      THREE.MathUtils.degToRad(8),
      THREE.MathUtils.degToRad(-18),
      0,
    );
    type FrameSample = {
      interval: number;
      submit: number;
      calls: number;
      triangles: number;
    };
    let comparison: {
      trial: number;
      frame: number;
      last: number;
      samples: { optimized: FrameSample[]; full: FrameSample[] };
      savedTurn: string;
      savedTilt: string;
      savedMode: string;
      invalid: boolean;
    } | null = null;
    const trialModes = ["optimized", "full", "full", "optimized"] as const;
    const percentile = (values: number[], fraction: number): number => {
      const sorted = [...values].sort((a, b) => a - b);
      return (
        sorted[
          Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))
        ] ?? 0
      );
    };
    document.addEventListener(
      "visibilitychange",
      () => {
        if (comparison && document.hidden) comparison.invalid = true;
      },
      { signal: lifetime.signal },
    );
    button("compare-deck").addEventListener(
      "click",
      () => {
        if (comparison) return;
        comparison = {
          trial: 0,
          frame: 0,
          last: 0,
          samples: { optimized: [], full: [] },
          savedTurn: turn.value,
          savedTilt: tilt.value,
          savedMode: deckMode.value,
          invalid: document.hidden,
        };
        for (const control of Object.values(settings)) control.disabled = true;
        view.value = "game";
        tilt.value = "8";
        comparisonOutput.value = "Comparing · warming up both modes…";
        requestDraw();
      },
      { signal: lifetime.signal },
    );
    let frameRequest = 0;
    lifetime.add(() => cancelAnimationFrame(frameRequest));
    function requestDraw(): void {
      if (!pending && !lifetime.disposed) {
        pending = true;
        frameRequest = requestAnimationFrame(draw);
      }
    }
    function draw(now: number): void {
      pending = false;
      if (lifetime.disposed) return;
      if (comparison) {
        deckMode.value = trialModes[comparison.trial] ?? "optimized";
        turn.value = String(
          Math.sin((comparison.frame / 90) * Math.PI * 2) * 45,
        );
      }
      const elapsed = Math.min((now - previous) / 1000, 0.1);
      previous = now;
      const blend = reducedMotion.matches ? 1 : 1 - Math.exp(-elapsed * 14),
        layoutView = view.value === "layout",
        gameView = view.value === "game",
        tableView = view.value === "table",
        edgeView = view.value === "edge";
      const targetX = tableView
          ? -Math.PI / 2
          : THREE.MathUtils.degToRad(Number(tilt.value)),
        targetY = tableView
          ? 0
          : edgeView
            ? 1.42
            : THREE.MathUtils.degToRad(Number(turn.value));
      rotation.x = THREE.MathUtils.lerp(rotation.x, targetX, blend);
      rotation.y = THREE.MathUtils.lerp(rotation.y, targetY, blend);
      currentFlip = THREE.MathUtils.lerp(currentFlip, targetFlip, blend);
      card.rotation.set(
        rotation.x,
        rotation.y + currentFlip,
        tableView ? THREE.MathUtils.degToRad(Number(turn.value)) : -0.025,
      );
      card.visible = !gameView && !layoutView;
      layout.group.visible = layoutView;
      gameTable.visible = gameView;
      const title = surface.title;
      if (title)
        title.textContent = layoutView
          ? "Classic board · Sample scene"
          : `${face.material.name || "Lightning Bolt"} · Card inspection`;
      const hint = surface.hint;
      if (hint)
        hint.textContent = layoutView
          ? options.playMode === "fixture-preview"
            ? "Sample hands · Drag to reorder or preview play. Click to inspect."
            : "Drag to reorder or request play. Click to inspect."
          : gameView
            ? "Drag to orbit. Click a card to read it."
            : "Drag the card to inspect its finish.";
      canvas.setAttribute(
        "aria-label",
        gameView
          ? `Three-dimensional table with three face-up cards, two face-down cards and a ${deckCount.value}-card deck.`
          : `Three-dimensional ${face.material.name || "Lightning Bolt"} card. Use the angle, finish and flip controls to inspect it.`,
      );
      if (gameView || layoutView) {
        face.material = frontMaterial;
        updateTable();
      }
      if (layoutView) {
        updateLayout();
        updateNavigation();
      }
      card.position.y = 0;
      if (tableView) {
        cardBounds.setFromObject(card);
        card.position.y = floor.position.y + 0.012 - cardBounds.min.y;
      }
      const rect = canvas.getBoundingClientRect();
      renderer.setSize(Math.round(rect.width), Math.round(rect.height), false);
      camera.aspect = rect.width / rect.height;
      camera.position.set(
        0,
        tableView ? 4.2 : 0.25,
        tableView
          ? Math.max(9.2, 5.8 / camera.aspect)
          : Math.max(7.8, 4.7 / camera.aspect),
      );
      if (state.sleeve && sleeveChoice.value === "rigid")
        camera.position.multiplyScalar(1.18);
      if (gameView) {
        const pileHeight =
          Number(deckCount.value) *
          (state.sleeve
            ? sleeveChoice.value === "rigid"
              ? 0.18
              : 0.032 * CARD_THICKNESS_SCALE
            : 0.022 * CARD_THICKNESS_SCALE);
        const framing =
          Math.max(1, 0.95 / camera.aspect, pileHeight / 10) /
          (Number(tableZoom.value) / 100);
        const orbit = THREE.MathUtils.degToRad(Number(turn.value) * 0.35);
        camera.position.set(
          Math.sin(orbit) * 18 * framing,
          (18 + Number(tilt.value) * 0.05) * framing,
          Math.cos(orbit) * 15 * framing,
        );
        const elevation = Math.max(0, pileHeight - 8) / 2;
        camera.position.y += elevation;
        camera.lookAt(0, floor.position.y + elevation, 0);
        key.shadow.camera.left = key.shadow.camera.bottom = -13;
        key.shadow.camera.right = key.shadow.camera.top = 13;
      } else {
        camera.lookAt(0, tableView ? -1.3 : 0, 0);
        key.shadow.camera.left = key.shadow.camera.bottom = -5;
        key.shadow.camera.right = key.shadow.camera.top = 5;
      }

      if (layoutView) {
        const focusYaw = layout.perspective(Number(playerFocus.value));
        const region = layout.bounds(Number(playerFocus.value)).clone();
        const isPlayerView = Number(playerFocus.value) !== 0;
        if (isPlayerView) {
          // Frame the owner's board low in view, with the opposing row beyond it.
          // The hand is camera-relative and does not enlarge the world-space bounds.
          if (Math.cos(focusYaw) < 0) region.max.z += 2;
          else region.min.z -= 2;
        }
        if (Number(playerFocus.value) !== 0 && handVisibility.checked) {
          // Apply the same owner-relative hand clearance to every focused board.
          region.translate(
            new THREE.Vector3(
              0,
              0,
              Math.cos(focusYaw) *
                (Math.min(2.5, Math.max(0, camera.aspect - 0.75) * 6) - 1.8),
            ),
          );
        }
        if (!isPlayerView)
          for (const seat of layout.seats) {
            const anchor = tableSeatAnchor(seat);
            region.expandByPoint(
              new THREE.Vector3(anchor.x, floor.position.y, anchor.z),
            );
          }
        region.getCenter(layoutCenter);
        region.getSize(layoutSize);
        const elevation = THREE.MathUtils.degToRad(
          isPlayerView ? 65 + Number(tilt.value) * 0.08 : 89.9,
        );
        const yaw =
          focusYaw + THREE.MathUtils.degToRad(Number(turn.value) * 0.12);
        const verticalSpan =
          layoutSize.z * Math.sin(elevation) +
          layoutSize.y * Math.cos(elevation);
        const distance =
          ((Math.max(layoutSize.x / camera.aspect, verticalSpan) /
            (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)))) *
            (isPlayerView ? 1.05 : 1.14)) /
          (Number(tableZoom.value) / 100);
        const pose = tableCamera.sample(
          { center: layoutCenter, yaw, elevation, distance },
          elapsed,
          reducedMotion.matches,
        );
        const facing = cardFacing.sample(
          readableCardTurn(
            pose.yaw - THREE.MathUtils.degToRad(Number(turn.value) * 0.12),
          ),
          elapsed,
          reducedMotion.matches,
        );
        facingFade.value = facing.opacity;
        layout.perspective(Number(playerFocus.value), facing.turn);
        if (facing.moving) requestDraw();
        camera.position
          .copy(pose.center)
          .add(
            new THREE.Vector3(
              Math.sin(pose.yaw) * pose.distance * Math.cos(pose.elevation),
              Math.sin(pose.elevation) * pose.distance,
              Math.cos(pose.yaw) * pose.distance * Math.cos(pose.elevation),
            ),
          );
        camera.lookAt(pose.center);
        key.shadow.camera.left = key.shadow.camera.bottom = -45;
        key.shadow.camera.right = key.shadow.camera.top = 45;
        canvas.setAttribute(
          "aria-label",
          `${playerCount.value}-player game table. Viewing ${Number(playerFocus.value) === 0 ? "the table overview" : playerFocus.value === viewerSeat.value ? "your board" : (surface.playerNames?.[Number(playerFocus.value) - 1] ?? `Player ${playerFocus.value}`)}.`,
        );
      }
      if (!layoutView) {
        tableCamera.reset();
        facingFade.value = 1;
      }
      if (layoutView && tableCamera.moving) requestDraw();
      key.shadow.camera.updateProjectionMatrix();
      fog.near = gameView || layoutView ? camera.position.length() * 0.85 : 8;
      fog.far = gameView || layoutView ? camera.position.length() + 40 : 24;
      if (layoutView) {
        fog.near = camera.position.length() + 100;
        fog.far = camera.position.length() + 200;
      }
      camera.near = gameView || layoutView ? 1 : 0.1;
      camera.far = layoutView ? 500 : gameView ? 150 : 60;
      camera.updateProjectionMatrix();
      camera.updateMatrixWorld();
      overview?.update(
        layout.seats.map((seat) => ({ ...seat, ...tableSeatAnchor(seat) })),
        Number(viewerSeat.value),
        Number(playerFocus.value) === 0,
        layoutView,
        inspection.active || piles.active || tableCamera.moving,
        (seat) => {
          const point = new THREE.Vector3(
            seat.x,
            floor.position.y + 0.6,
            seat.z,
          ).project(camera);
          return {
            x: ((point.x + 1) * rect.width) / 2,
            y: ((1 - point.y) * rect.height) / 2,
          };
        },
      );
      const viewingOpponent =
        Number(playerFocus.value) !== 0 &&
        playerFocus.value !== viewerSeat.value;
      hands.viewOpponent(viewingOpponent ? Number(playerFocus.value) : 0);
      const handMoving = hands.frame(
        layoutView &&
          Number(playerFocus.value) !== 0 &&
          handVisibility.checked &&
          !tableCamera.moving,
        Number(playerFocus.value),
        elapsed,
        reducedMotion.matches,
      );
      hands.viewedHand.visible = hands.viewedExternal.visible =
        layoutView &&
        viewingOpponent &&
        handVisibility.checked &&
        !tableCamera.moving;
      if (viewingOpponent) {
        hands.local.visible = false;
        hands.external.visible = false;
        hands.opponents.visible = false;
        hands.opponentExternal.visible = false;
      }
      const cardMoving = inspection.tick(elapsed, reducedMotion.matches, 0);
      hands.setBrowsingBlocked(inspection.active || piles.active);
      if (piles.frame(rect.width, elapsed, reducedMotion.matches))
        requestDraw();
      opponentNavigation.hidden =
        !layoutView ||
        Number(playerFocus.value) === 0 ||
        piles.active ||
        inspection.active ||
        tableCamera.moving;
      returnBoard.hidden =
        !layoutView || !viewingOpponent || inspection.active || piles.active;
      tableNavigation.setVisible(
        layoutView && Number(playerFocus.value) !== 0 && !piles.active,
        tableCamera.moving,
        inspection.active,
      );
      if (tableNavigation.tick(elapsed, reducedMotion.matches)) requestDraw();
      if (plays.tick(layoutView, elapsed, reducedMotion.matches)) requestDraw();
      stackPanel.hidden =
        !layoutView || piles.active || inspection.active || plays.count === 0;
      stackCount.value = `Stack · ${plays.count} pending ${plays.count === 1 ? "spell" : "spells"}`;
      if (resolvePreview) resolvePreview.disabled = !plays.canResolve;
      handSummary.value = `Your hand: ${hands.local.children.length} · Each opponent: ${opponentHandSize.value}. Sample cards only.`;
      if (piles.active) canvas.setAttribute("aria-label", piles.description);
      if (inspection.active)
        canvas.setAttribute(
          "aria-label",
          `Reading ${inspection.label} over the table. Press Escape to return it.`,
        );
      if (handMoving || cardMoving) requestDraw();
      const angle = THREE.MathUtils.degToRad(Number(light.value));
      key.position.set(Math.sin(angle) * 6, 5, Math.cos(angle) * 6);
      foilLight.value
        .copy(key.position)
        .transformDirection(camera.matrixWorldInverse);
      finishUniform.value = Number(state.foil);
      for (const material of [
        frontMaterial,
        mountainMaterial,
        ...boardMaterials.values(),
      ]) {
        material.roughness = state.foil ? 0.29 : 0.76;
        material.specularIntensity = state.foil ? 0.8 : 0.18;
        material.clearcoat = state.foil ? 0.45 : 0.035;
        material.clearcoatRoughness = state.foil ? 0.28 : 0.6;
        material.envMapIntensity = state.foil ? 0.55 : 0.16;
      }
      const submitStart = performance.now();
      renderer.render(scene, camera);
      surface.onFrame?.();
      const submitMilliseconds = performance.now() - submitStart;
      if (comparison) {
        const mode = trialModes[comparison.trial] ?? "optimized";
        if (comparison.frame >= 20 && comparison.last > 0) {
          comparison.samples[mode].push({
            interval: now - comparison.last,
            submit: submitMilliseconds,
            calls: renderer.info.render.calls,
            triangles: renderer.info.render.triangles,
          });
        }
        if (
          document.hidden ||
          (comparison.last > 0 && now - comparison.last > 250)
        )
          comparison.invalid = true;
        comparison.last = now;
        comparison.frame++;
        if (comparison.frame >= 90) {
          comparison.trial++;
          comparison.frame = 0;
          comparison.last = 0;
        }
        if (comparison.trial === trialModes.length) {
          const lines = (["optimized", "full"] as const).map((mode) => {
            const samples = comparison?.samples[mode] ?? [];
            return `${mode === "optimized" ? "Optimized" : "Full cards"}: frame ${percentile(
              samples.map((s) => s.interval),
              0.5,
            ).toFixed(1)} / ${percentile(
              samples.map((s) => s.interval),
              0.95,
            ).toFixed(1)} ms; submit ${percentile(
              samples.map((s) => s.submit),
              0.5,
            ).toFixed(2)} ms; ${Math.round(
              percentile(
                samples.map((s) => s.calls),
                0.5,
              ),
            )} draws; ${Math.round(
              percentile(
                samples.map((s) => s.triangles),
                0.5,
              ),
            ).toLocaleString()} triangles.`;
          });
          comparisonOutput.value = `${comparison.invalid ? "Interrupted run; rerun for valid timing. " : ""}${deckCount.value} cards · ${state.sleeve ? (sleeveChoice.selectedOptions[0]?.textContent ?? "Sleeved") : "Unsleeved"}. ${lines.join("\n")}\nFrame = median / p95, including display scheduling. Submit = CPU only; not GPU time.`;
          turn.value = comparison.savedTurn;
          tilt.value = comparison.savedTilt;
          deckMode.value = comparison.savedMode;
          comparison = null;
          for (const control of Object.values(settings))
            control.disabled = false;
        } else {
          comparisonOutput.value = `Comparing · trial ${comparison.trial + 1}/4 · ${mode === "optimized" ? "Optimized" : "Full cards"}`;
        }
        requestDraw();
      }
      if (
        Math.abs(rotation.x - targetX) +
          Math.abs(rotation.y - targetY) +
          Math.abs(currentFlip - targetFlip) >
        0.0001
      )
        requestDraw();
    }
    for (const control of [
      tilt,
      turn,
      light,
      view,
      tableZoom,
      deckCount,
      deckMode,
      playerCount,
      playerFocus,
      navigationMode,
      viewerSeat,
      tableMode,
      handSize,
      opponentHandSize,
      handVisibility,
    ])
      control.addEventListener(
        "input",
        () => {
          inspection.cancel();
          piles.close();
          if (control === navigationMode) {
            attackRecipient = null;
            playerFocus.value = viewerSeat.value;
          }
          if (
            (control === deckCount ||
              control === deckMode ||
              control === tableZoom) &&
            view.value !== "layout"
          )
            view.value = "game";
          if (control === tableMode) {
            if (tableMode.value === "two-two") playerCount.value = "4";
            if (tableMode.value === "one-two" || tableMode.value === "two-one")
              playerCount.value = "3";
            playerFocus.value = viewerSeat.value;
          }
          if (control === playerCount) {
            if (
              (tableMode.value === "two-two" && playerCount.value !== "4") ||
              ((tableMode.value === "one-two" ||
                tableMode.value === "two-one") &&
                playerCount.value !== "3")
            )
              tableMode.value = "ffa";
            playerFocus.value = viewerSeat.value;
          }
          if (control === viewerSeat) playerFocus.value = viewerSeat.value;
          if (
            control === playerCount ||
            control === playerFocus ||
            control === navigationMode ||
            control === viewerSeat ||
            control === tableMode ||
            control === handSize ||
            control === opponentHandSize ||
            control === handVisibility
          )
            view.value = "layout";
          requestDraw();
        },
        { signal: lifetime.signal },
      );
    for (const [control, isFoil] of [
      [ordinary, false],
      [foil, true],
    ] as const)
      control.addEventListener(
        "click",
        () => {
          state.foil = isFoil;
          ordinary.setAttribute("aria-pressed", String(!isFoil));
          foil.setAttribute("aria-pressed", String(isFoil));
          requestDraw();
        },
        { signal: lifetime.signal },
      );
    cardBackChoice.addEventListener(
      "change",
      () => {
        const choice = z
          .enum([
            "classic",
            "spellwake",
            "unlettered",
            ...supporterNames,
            "initiate-foil",
            "adept-foil",
            "mage-foil",
            "archmage-foil",
            "benefactor-foil",
          ])
          .parse(cardBackChoice.value);
        const supporter = supporterSchema.safeParse(
          choice.replace(/-foil$/, ""),
        );
        const isFoilBack = supporter.success && choice.endsWith("-foil");
        if (supporter.success) {
          reverse.material.map = supporterArt.get(supporter.data) ?? null;
          reverseFoil.mask.value = supporterMasks.get(supporter.data);
        } else {
          const defaultChoice = z
            .enum(["classic", "spellwake", "unlettered"])
            .parse(choice);
          reverse.material.map = {
            classic: back,
            spellwake: midnightArt,
            unlettered: unletteredBackArt,
          }[defaultChoice];
        }
        reverseFoil.amount.value = Number(isFoilBack);
        reverse.material.roughness = 0.65;
        reverse.material.envMapIntensity = isFoilBack ? 0.85 : 0.3;
        reverse.material.needsUpdate = true;
        requestDraw();
      },
      { signal: lifetime.signal },
    );
    sleeveChoice.addEventListener(
      "change",
      () => {
        const choice = sleeveDesignSchema.parse(sleeveChoice.value);
        const supporter = supporterSchema.safeParse(choice);
        const supporterTexture = supporter.success
          ? supporterArt.get(supporter.data)
          : undefined;
        const supporterMask =
          supporter.success && choice !== "initiate" && choice !== "adept"
            ? supporterMasks.get(supporter.data)
            : undefined;
        sleeveFoil.amount.value = supporterMask ? 1 : 0;
        sleeveFoil.mask.value =
          supporterMask ?? supporterMasks.get("benefactor");
        const isMatte = choice === "black-matte" || choice === "initiate";
        matteBackUniform.value = Number(isMatte);
        const isClear = choice === "penny" || choice === "rigid";
        const isRigid = choice === "rigid";
        const isGloss =
          choice === "black-gloss" ||
          choice === "dragon-gloss" ||
          (supporter.success && !isMatte);
        const hasArtwork = choice === "dragon-gloss";
        sleeve.scale.set(
          isRigid ? 3 / 2.62 : isClear ? 1.02 : 1,
          isRigid ? 4 / 3.65 : isClear ? 1.03 : 1,
          isRigid ? 3.2 : CARD_THICKNESS_SCALE,
        );
        setSleeveProfile(isRigid);
        sleeveStock.visible = !isClear;
        sleeveStock.castShadow = !isClear;
        const backMaterial = sleeveBack.material;
        backMaterial.map =
          supporterTexture ??
          (choice === "midnight" ? midnightArt : hasArtwork ? dragonArt : null);
        backMaterial.color.setHex(
          isClear
            ? 0xf0f7ff
            : choice === "black-matte" || choice === "black-gloss"
              ? isMatte
                ? 0x303133
                : 0x101112
              : 0xffffff,
        );
        backMaterial.transparent = isClear;
        backMaterial.opacity = isClear ? (isRigid ? 0.055 : 0.025) : 1;
        backMaterial.depthWrite = !isClear;
        backMaterial.roughness = isMatte
          ? 0.82
          : isClear
            ? 0.14
            : isGloss
              ? 0.42
              : 0.59;
        backMaterial.clearcoat = supporterMask
          ? 0.035
          : isMatte
            ? 0.14
            : isGloss
              ? 0.2
              : isClear
                ? 0.5
                : 0.12;
        if (supporterMask) backMaterial.roughness = 0.65;
        backMaterial.clearcoatRoughness = isMatte || isGloss ? 0.4 : 0.48;
        backMaterial.specularIntensity = isMatte ? 0.55 : isGloss ? 0.35 : 0.3;
        backMaterial.envMapIntensity = supporterMask
          ? 0.85
          : isMatte
            ? 0.45
            : isGloss
              ? 0.35
              : 0.3;
        backMaterial.normalMap = isMatte ? matteNormal : null;
        backMaterial.normalScale.setScalar(isMatte ? 0.9 : 1);
        backMaterial.roughnessMap = isMatte ? matteRoughness : null;
        backMaterial.needsUpdate = true;
        const edgeColor = isClear
          ? 0xc3d9eb
          : supporter.success
            ? choice === "benefactor"
              ? 0x321c24
              : 0x182233
            : choice === "midnight"
              ? 0x283449
              : 0x101112;
        for (const material of [rim.material, seams.material]) {
          material.color.setHex(edgeColor);
          material.transparent = isClear;
          material.opacity = isClear ? (isRigid ? 0.32 : 0.12) : 1;
          material.depthWrite = !isClear;
          material.roughness = isClear ? 0.15 : isMatte ? 0.8 : 0.35;
          material.needsUpdate = true;
        }
        // Compression smooths the grain at the weld without painting a heavy black frame.
        if (choice === "black-matte")
          seams.material.color.copy(backMaterial.color);
        if (isMatte) seams.material.roughness = 0.68;
        seams.castShadow = !isClear;
        sleeveStock.material.color.setHex(edgeColor);
        sleeveFront.material.opacity = isClear
          ? isRigid
            ? 0.05
            : 0.025
          : 0.035;
        sleeveFront.material.roughness = isClear ? 0.14 : 0.32;
        const attribution = surface.sleeveAttribution;
        if (attribution) attribution.hidden = !hasArtwork;
        requestDraw();
      },
      { signal: lifetime.signal },
    );
    sleeveToggle.addEventListener(
      "click",
      () => {
        state.sleeve = !state.sleeve;
        sleeve.visible = state.sleeve;
        if (state.sleeve) card.add(sleeve);
        else card.remove(sleeve);
        sleeveToggle.setAttribute("aria-pressed", String(state.sleeve));
        sleeveToggle.textContent = state.sleeve ? "Sleeve on" : "Sleeve off";
        requestDraw();
      },
      { signal: lifetime.signal },
    );
    flip.addEventListener(
      "click",
      () => {
        state.back = !state.back;
        targetFlip = state.back ? Math.PI : 0;
        flip.setAttribute("aria-pressed", String(state.back));
        flip.textContent = state.back ? "Show front" : "Show back";
        requestDraw();
      },
      { signal: lifetime.signal },
    );
    button("reset").addEventListener(
      "click",
      () => {
        face.material = frontMaterial;
        tilt.value = "8";
        turn.value = "-18";
        light.value = "-40";
        view.value = "inspect";
        state.back = false;
        targetFlip = 0;
        flip.setAttribute("aria-pressed", "false");
        flip.textContent = "Show back";
        requestDraw();
      },
      { signal: lifetime.signal },
    );
    let drag: {
      x: number;
      y: number;
      tilt: number;
      turn: number;
      hand: boolean;
      pile: boolean;
      pileMoved: boolean;
      pileTravel: number;
      time: number;
      handFold: boolean;
      handIndex: number;
      navigationSeat: number | null;
      lastX: number;
      lastY: number;
    } | null = null;
    function pointerRay(event: PointerEvent | WheelEvent): THREE.Raycaster {
      const rect = canvas.getBoundingClientRect();
      const ray = new THREE.Raycaster();
      ray.setFromCamera(
        new THREE.Vector2(
          ((event.clientX - rect.left) / rect.width) * 2 - 1,
          (-(event.clientY - rect.top) / rect.height) * 2 + 1,
        ),
        camera,
      );
      return ray;
    }
    canvas.addEventListener(
      "pointerdown",
      (event) => {
        if (comparison || inspection.active || tableCamera.moving) return;
        drag = {
          x: event.clientX,
          y: event.clientY,
          tilt: Number(tilt.value),
          turn: Number(turn.value),
          pile: piles.active,
          pileMoved: false,
          pileTravel: 0,
          time: event.timeStamp,
          lastX: event.clientX,
          lastY: event.clientY,
          handFold: false,
          handIndex: hands.hit(pointerRay(event)),
          navigationSeat:
            view.value === "layout"
              ? tableNavigation.hit(pointerRay(event))
              : null,
          hand:
            !piles.active &&
            view.value === "layout" &&
            handVisibility.checked &&
            hands.beginDrag(hands.hit(pointerRay(event))),
        };
        if (drag.hand && hands.canBrowse(drag.handIndex)) {
          drag.handFold = true;
          hands.beginPan(drag.handIndex);
        }
        if (piles.active) piles.beginPan();
        canvas.setPointerCapture(event.pointerId);
      },
      { signal: lifetime.signal },
    );
    canvas.addEventListener(
      "pointermove",
      (event) => {
        if (inspection.active) return;
        if (!drag) {
          if (
            view.value === "layout" &&
            tableNavigation.hover(pointerRay(event))
          )
            requestDraw();
          if (piles.active) {
            if (piles.hover(pointerRay(event))) requestDraw();
            return;
          }
          if (view.value === "layout" && handVisibility.checked) {
            const rect = canvas.getBoundingClientRect();
            const ray = new THREE.Raycaster();
            ray.setFromCamera(
              new THREE.Vector2(
                ((event.clientX - rect.left) / rect.width) * 2 - 1,
                (-(event.clientY - rect.top) / rect.height) * 2 + 1,
              ),
              camera,
            );
            const index = hands.pick(ray);
            if (hands.hover(index)) requestDraw();
          }
          return;
        }
        if (drag.navigationSeat !== null) {
          if (tableNavigation.hover(pointerRay(event))) requestDraw();
          return;
        }
        if (drag.pile) {
          const distance = drag.x - event.clientX;
          drag.pileTravel += Math.abs(distance);
          if (drag.pileTravel > 6) drag.pileMoved = true;
          piles.pan(distance, (event.timeStamp - drag.time) / 1000);
          drag.x = event.clientX;
          drag.time = event.timeStamp;
          return;
        }
        if (drag.hand && drag.handFold) {
          if (
            isHandLift(
              drag.y,
              drag.lastX,
              drag.lastY,
              event.clientX,
              event.clientY,
            )
          ) {
            hands.endPan(true);
            if (drag.pileTravel > 6) drag.handIndex = hands.currentIndex;
            hands.beginDrag(drag.handIndex);
            drag.x = event.clientX;
            drag.y = event.clientY;
            drag.handFold = false;
          } else {
            drag.pileTravel += Math.abs(drag.lastX - event.clientX);
            if (drag.pileTravel > 6)
              hands.pan(
                drag.lastX - event.clientX,
                canvas.clientWidth,
                (event.timeStamp - drag.time) / 1000,
              );
            drag.lastX = event.clientX;
            drag.lastY = event.clientY;
            drag.time = event.timeStamp;
            requestDraw();
            return;
          }
        }
        if (drag.hand) {
          if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) > 6) {
            hands.dragTo(pointerRay(event));
            interactionStatus.value =
              hands.canPlayDragged &&
              event.clientY <
                canvas.getBoundingClientRect().top + canvas.clientHeight * 0.67
                ? options.playMode === "fixture-preview"
                  ? "Release on table to preview play"
                  : "Release on table to request play"
                : "Move sideways to reorder";
            requestDraw();
          }
          return;
        }
        // Empty-space drags in play are not camera gestures. Seat arrows own camera travel.
        if (view.value === "layout") return;
        if (view.value !== "game") view.value = "inspect";
        turn.value = String(
          THREE.MathUtils.clamp(
            drag.turn + (event.clientX - drag.x) * 0.25,
            -75,
            75,
          ),
        );
        tilt.value = String(
          THREE.MathUtils.clamp(
            drag.tilt + (event.clientY - drag.y) * 0.25,
            -65,
            65,
          ),
        );
        requestDraw();
      },
      { signal: lifetime.signal },
    );
    canvas.addEventListener(
      "pointerleave",
      () => {
        if (tableNavigation.hover(null)) requestDraw();
        if (!drag?.hand && hands.hover(-1)) requestDraw();
      },
      { signal: lifetime.signal },
    );
    const dismissInspection = (): void => {
      hands.hover(-1);
      if (inspection.active) inspection.close();
      else piles.close();
      canvas.focus();
      requestDraw();
    };

    document.addEventListener(
      "keydown",
      (event) => {
        if (event.key === "Escape") {
          if (inspection.active || piles.active) dismissInspection();
          if (drag?.pile) drag = null;
          if (drag?.hand) {
            hands.endPan(true);
            hands.endDrag(null);
            drag = null;
            interactionStatus.value = "Drag canceled";
            requestDraw();
          }
        }
      },
      { signal: lifetime.signal },
    );
    canvas.tabIndex = 0;
    canvas.addEventListener(
      "wheel",
      (event) => {
        if (piles.active && !inspection.active) {
          event.preventDefault();
          piles.pan(event.deltaX || event.deltaY, 1 / 60);
          piles.endPan(true);
        } else if (view.value === "layout") {
          const index = hands.pick(pointerRay(event));
          if (index >= 0 && hands.canBrowse(index)) {
            event.preventDefault();
            hands.beginPan(index);
            hands.pan(event.deltaX || event.deltaY, canvas.clientWidth, 1 / 60);
            hands.endPan(true);
            requestDraw();
          }
        }
      },
      { passive: false, signal: lifetime.signal },
    );
    canvas.addEventListener(
      "keydown",
      (event) => {
        if (
          piles.active &&
          !inspection.active &&
          (event.key === "ArrowLeft" || event.key === "ArrowRight")
        ) {
          event.preventDefault();
          piles.move(event.key === "ArrowLeft" ? -1 : 1);
          return;
        }
        if (piles.active) {
          if (event.key === "Enter" && !inspection.active) {
            event.preventDefault();
            piles.readSelected();
            requestDraw();
          }
          return;
        }
        if (
          view.value !== "layout" ||
          !handVisibility.checked ||
          inspection.active
        )
          return;
        if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
          event.preventDefault();
          hands.browse(event.key === "ArrowLeft" ? -1 : 1);
          requestDraw();
        }
        if (event.key === "Enter" && hands.currentCard) {
          event.preventDefault();
          inspection.open(hands.currentCard);
          requestDraw();
        }
      },
      { signal: lifetime.signal },
    );
    inspectHand.addEventListener(
      "click",
      () => {
        const selected = hands.currentCard;
        if (selected && view.value === "layout") {
          inspection.open(selected);
          requestDraw();
        }
      },
      { signal: lifetime.signal },
    );
    canvas.addEventListener(
      "pointerup",
      (event) => {
        if (
          drag?.navigationSeat !== null &&
          drag?.navigationSeat !== undefined
        ) {
          const seat = drag.navigationSeat;
          const clicked =
            Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 6;
          drag = null;
          if (clicked) activatePlayerControl(seat);
          return;
        }
        if (inspection.active) {
          dismissInspection();
          drag = null;
          return;
        }
        if (piles.active) {
          if (drag?.pileMoved) piles.endPan(event.timeStamp - drag.time > 80);
          else {
            piles.endPan(true);
            if (!piles.read(pointerRay(event))) piles.close();
          }
          drag = null;
          requestDraw();
          return;
        }
        if (drag?.hand && drag.handFold) {
          hands.endPan(event.timeStamp - drag.time > 80);
          if (drag.pileTravel < 6) {
            const card = hands.cardAt(drag.handIndex);
            if (card) inspection.open(card);
          }
          drag = null;
          requestDraw();
          return;
        }
        if (drag?.hand) {
          const moved =
            Math.hypot(event.clientX - drag.x, event.clientY - drag.y) > 6;
          const rect = canvas.getBoundingClientRect();
          const onTable =
            moved &&
            hands.canPlayDragged &&
            event.clientY < rect.top + rect.height * 0.67 &&
            event.clientX >= rect.left &&
            event.clientX <= rect.right &&
            event.clientY >= rect.top;
          const card = hands.endDrag(
            onTable || !moved ? null : hands.slotAt(pointerRay(event)),
          );
          if (card && onTable) {
            void interaction
              .drop({
                presentationCardId: card.uuid,
                ...(typeof card.userData.sourceId === "string"
                  ? { sourceId: card.userData.sourceId }
                  : {}),
              })
              .then((result) => {
                if (lifetime.disposed) return;
                if (result.status === "failed")
                  console.error("Table play intent failed", result.error);
                interactionStatus.value =
                  result.status === "preview"
                    ? "Play preview"
                    : result.status === "requested"
                      ? "Play requested"
                      : result.status === "unavailable"
                        ? "Match connection unavailable"
                        : result.status === "failed"
                          ? "Unable to request play"
                          : "";
                requestDraw();
              });
          } else if (moved) interactionStatus.value = "Hand reordered";
          else if (card) inspection.open(card);
          drag = null;
          requestDraw();
          return;
        }
        if (
          drag &&
          (view.value === "game" || view.value === "layout") &&
          Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 6
        ) {
          const rect = canvas.getBoundingClientRect();
          const ray = new THREE.Raycaster();
          ray.setFromCamera(
            new THREE.Vector2(
              ((event.clientX - rect.left) / rect.width) * 2 - 1,
              (-(event.clientY - rect.top) / rect.height) * 2 + 1,
            ),
            camera,
          );
          const hit = ray
            .intersectObjects(
              view.value === "layout"
                ? [
                    ...(handVisibility.checked
                      ? [
                          hands.local,
                          hands.external,
                          hands.viewedHand,
                          hands.viewedExternal,
                        ]
                          .filter((group) => group.visible)
                          .flatMap((group) => group.children)
                      : []),
                    ...hands.opponentExternal.children.filter(
                      (card) => hands.opponentExternal.visible && card.visible,
                    ),
                    ...layout.group.children,
                    ...plays.group.children,
                  ]
                : laidOutCards.children,
              true,
            )
            .find((hit) => {
              let node: THREE.Object3D | null = hit.object;
              while (node) {
                if (
                  node.userData.isStudyCard === true ||
                  typeof node.userData.pileKind === "string"
                )
                  return true;
                node = node.parent;
              }
              return false;
            });
          if (hit) {
            let pile: THREE.Object3D | null = hit.object;
            while (pile && typeof pile.userData.pileKind !== "string")
              pile = pile.parent;
            if (pile) {
              interactionStatus.value = "";
              piles.open(pile);
              drag = null;
              requestDraw();
              return;
            }
            let selected = hit.object;
            while (selected.parent && selected.userData.isStudyCard !== true)
              selected = selected.parent;
            if (!(selected instanceof THREE.InstancedMesh)) {
              hands.hover(-1);
              interactionStatus.value = "";
              inspection.open(selected);
            }
            requestDraw();
          }
        }
        drag = null;
      },
      { signal: lifetime.signal },
    );
    for (const name of ["pointercancel", "lostpointercapture"])
      canvas.addEventListener(
        name,
        () => {
          piles.endPan(true);
          hands.endPan(true);
          hands.endDrag(null);
          drag = null;
          requestDraw();
        },
        { signal: lifetime.signal },
      );
    const resizeObserver = new ResizeObserver(requestDraw);
    resizeObserver.observe(canvas);
    lifetime.add(() => resizeObserver.disconnect());
    reducedMotion.addEventListener("change", requestDraw, {
      signal: lifetime.signal,
    });
    cardBackChoice.dispatchEvent(new Event("change"));
    sleeveChoice.dispatchEvent(new Event("change"));
    if (sleeveToggle.checked) sleeveToggle.dispatchEvent(new Event("click"));
    if (foil.checked) foil.dispatchEvent(new Event("click"));
    status.textContent = surface.readyMessage ?? "Ready · Three.js / WebGL2";
    requestDraw();
    return {
      dispose: lifetime.dispose,
      get disposed() {
        return lifetime.disposed;
      },
    };
  } catch (error: unknown) {
    lifetime.dispose();
    throw error;
  }
}
