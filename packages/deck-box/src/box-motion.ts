import * as THREE from "three";
import { createBoxSurface } from "./box-renderer";
import { createBoxTextures, type BoxTextures } from "./box-textures";
import type { BoxAppearance } from "./box-appearance";
import { DEFAULT_BOX_VIEW, type BoxView } from "./box-view";
import { createLeatherBend, type LeatherBend } from "./leather-bend";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { clone as cloneSkinned } from "three/addons/utils/SkeletonUtils.js";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

export type BoxMotion = {
  getView: () => BoxView;
  setAppearance: (appearance: BoxAppearance) => void;
  setOpen: (open: boolean) => void;
  setInspection: (enabled: boolean) => void;
  setSeparated: (separated: boolean) => void;
  setFold: (joint: "rear" | "front", degrees: number) => void;
  setGuides: (visible: boolean) => void;
  reset: () => void;
  dispose: () => void;
};
/** Subtle hover reveal using the approved continuous leather deformation. */
export async function mountBoxMotion(
  host: HTMLElement,
  url: string,
  options: {
    /** Public folder holding the box's texture inputs; see `box-textures`. */
    artworkUrl: string;
    collection?: boolean;
    appearance?: BoxAppearance;
    view?: BoxView | undefined;
    signal?: AbortSignal;
  },
): Promise<BoxMotion> {
  options.signal?.throwIfAborted();
  // Every box is drawn by the page's one renderer into its own canvas.
  const surface = createBoxSurface(
    Math.min(window.devicePixelRatio, options.collection ? 1.5 : 2),
    () => draw(),
  );
  const canvas = surface.canvas;
  canvas.setAttribute(
    "aria-label",
    options.collection
      ? "Animated leather deck box"
      : "3D deck box; drag to rotate and scroll to zoom",
  );
  canvas.setAttribute("role", "img");
  host.append(canvas);
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(30, 1, 0.01, 100);
  const controls = new OrbitControls(camera, canvas);
  controls.enabled = !options.collection;
  if (options.collection) canvas.style.touchAction = "pan-y";
  controls.enablePan = false;
  controls.minDistance = 2.3;
  controls.maxDistance = 9;
  controls.maxPolarAngle = Math.PI * 0.49;
  scene.environmentIntensity = 0.45;
  scene.add(new THREE.HemisphereLight(0xd5e4ff, 0x3d3028, 0.7));
  const key = new THREE.DirectionalLight(0xffe1b5, 2.5);
  key.position.set(-3, 5, 4);
  key.castShadow = true;
  key.shadow.mapSize.set(1024, 1024);
  key.shadow.camera.left = -3;
  key.shadow.camera.right = 3;
  key.shadow.camera.top = 3;
  key.shadow.camera.bottom = -3;
  key.shadow.normalBias = 0.015;
  scene.add(key);
  const rim = new THREE.DirectionalLight(0xa4bfff, 1.4);
  rim.position.set(3, 3, -3);
  scene.add(rim);
  const ground = new THREE.Mesh(
    new THREE.PlaneGeometry(200, 200),
    new THREE.ShadowMaterial({ opacity: 0.28 }),
  );
  ground.rotation.x = -Math.PI / 2;
  ground.position.y = -0.965;
  ground.receiveShadow = true;
  scene.add(ground);
  let disposed = false;
  let body: THREE.Object3D | undefined;
  let flap: THREE.Object3D | undefined;
  let leather: LeatherBend | undefined;
  let textures: BoxTextures | undefined;
  const rear = new THREE.Group();
  const front = new THREE.Group();
  let poseScale = 1;
  let poseTarget = options.collection ? 0 : 0.2;
  let separated = false;
  let inspection = false;
  let pinned = false;
  let hovered = false;
  let progress = 0;
  let velocity = 0;
  let animationFrame = 0;
  let previousTime = 0;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let rearDegrees = 0;
  let frontDegrees = 0;
  const guides: THREE.Object3D[] = [];
  let frame = 0;
  const draw = (): void => {
    if (disposed || frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (!disposed) surface.render(scene, camera);
    });
  };
  const pose = (scheduleDraw = true): void => {
    if (disposed) return;
    if (body) body.position.x = separated ? -1.1 : 0;
    if (flap) flap.position.x = separated ? 1.1 : 0;
    if (leather) {
      const frames = leather.pose(rearDegrees, frontDegrees);
      rear.position.set(0, frames.rear.y, frames.rear.z);
      front.position.set(0, frames.front.y, frames.front.z);
      if (inspection) {
        const nextScale = Math.max(1, (frames.highest + 0.95) / 2.3);
        poseTarget = 0.2 + Math.max(0, frames.highest - 0.95) / 2;
        camera.position
          .sub(controls.target)
          .multiplyScalar(nextScale / poseScale);
        controls.target.set(0, poseTarget, 0);
        camera.position.add(controls.target);
        poseScale = nextScale;
        controls.maxDistance = 9 * framingScale * poseScale;
        controls.update();
      }
    }
    host.dataset.separated = String(separated);
    host.dataset.rearFold = String(rearDegrees);
    host.dataset.frontFold = String(frontDegrees);
    host.dataset.openProgress = String(progress);
    host.dataset.hovered = String(hovered);
    host.dataset.inspection = String(inspection);
    if (scheduleDraw) draw();
  };
  const animate = (now: number): void => {
    animationFrame = 0;
    if (disposed || inspection) return;
    const target = pinned || hovered ? 1 : 0;
    const elapsed = Math.min((now - previousTime) / 1000, 0.05);
    previousTime = now;
    // Critically damped response keeps velocity continuous on pointer reversals.
    const frequency = target ? 18 : 23;
    const offset = progress - target;
    const coefficient = velocity + frequency * offset;
    const decay = Math.exp(-frequency * elapsed);
    progress = target + (offset + coefficient * elapsed) * decay;
    velocity = (velocity - frequency * coefficient * elapsed) * decay;
    if (progress < 0 || progress > 1) {
      progress = Math.max(0, Math.min(1, progress));
      velocity = 0;
    }
    if (Math.abs(progress - target) < 0.0005 && Math.abs(velocity) < 0.005) {
      progress = target;
      velocity = 0;
    }
    rearDegrees = 8 * progress;
    frontDegrees = 12 * progress;
    pose(false);
    surface.render(scene, camera);
    if (progress !== target || velocity !== 0)
      animationFrame = requestAnimationFrame(animate);
  };
  const transition = (): void => {
    if (inspection || disposed || document.hidden) return;
    if (reducedMotion.matches) {
      cancelAnimationFrame(animationFrame);
      animationFrame = 0;
      progress = pinned || hovered ? 1 : 0;
      velocity = 0;
      rearDegrees = 8 * progress;
      frontDegrees = 12 * progress;
      pose();
    } else if (!animationFrame) {
      previousTime = performance.now();
      animationFrame = requestAnimationFrame(animate);
    }
  };
  const setAppearance = (appearance: BoxAppearance): void => {
    if (disposed) return;
    textures?.repaint(appearance);
    draw();
  };
  const setOpen = (open: boolean): void => {
    if (disposed) return;
    pinned = open;
    transition();
  };
  // Fixed closed/open envelope avoids moving-cover hit-test feedback.
  const hitBox = new THREE.Mesh(
    new THREE.BoxGeometry(1.68, 2.08, 1.65),
    new THREE.MeshBasicMaterial(),
  );
  hitBox.position.set(0, 0.08, 0.225);
  hitBox.updateMatrixWorld();
  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  const pointerMove = (event: PointerEvent): void => {
    if (inspection || event.pointerType === "touch" || event.buttons !== 0)
      return;
    const rect = canvas.getBoundingClientRect();
    pointer.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      (-(event.clientY - rect.top) / rect.height) * 2 + 1,
    );
    raycaster.setFromCamera(pointer, camera);
    const next = raycaster.intersectObject(hitBox).length > 0;
    if (next !== hovered) {
      hovered = next;
      transition();
    }
  };
  const pointerLeave = (): void => {
    hovered = false;
    transition();
  };
  const visibilityChange = (): void => {
    if (document.hidden) {
      hovered = false;
      cancelAnimationFrame(animationFrame);
      animationFrame = 0;
      velocity = 0;
    } else {
      transition();
    }
  };
  canvas.addEventListener("pointermove", pointerMove);
  canvas.addEventListener("pointerleave", pointerLeave);
  document.addEventListener("visibilitychange", visibilityChange);
  reducedMotion.addEventListener("change", transition);
  const setInspection = (enabled: boolean): void => {
    if (disposed) return;
    inspection = enabled;
    cancelAnimationFrame(animationFrame);
    animationFrame = 0;
    progress = 0;
    velocity = 0;
    hovered = false;
    pinned = false;
    rearDegrees = 0;
    frontDegrees = 0;
    separated = enabled;
    poseScale = 1;
    poseTarget = 0.2;
    setGuides(enabled);
    pose();
    reset();
  };
  const setSeparated = (value: boolean): void => {
    if (disposed) return;
    separated = value;
    pose();
  };
  const setFold = (joint: "rear" | "front", degrees: number): void => {
    if (disposed) return;
    if (joint === "rear") rearDegrees = degrees;
    else frontDegrees = degrees;
    pose();
  };
  const setGuides = (visible: boolean): void => {
    if (disposed) return;
    for (const guide of guides) guide.visible = visible;
    draw();
  };
  const addJointGuide = (joint: THREE.Object3D, color: number): void => {
    const line = new THREE.Mesh(
      new THREE.CylinderGeometry(0.013, 0.013, 1.86, 12),
      new THREE.MeshBasicMaterial({ color, depthTest: false }),
    );
    line.rotation.z = Math.PI / 2;
    line.renderOrder = 10;
    joint.add(line);
    guides.push(line);
    for (const x of [-0.93, 0.93]) {
      const dot = new THREE.Mesh(
        new THREE.SphereGeometry(0.045, 16, 12),
        new THREE.MeshBasicMaterial({ color, depthTest: false }),
      );
      dot.position.x = x;
      dot.renderOrder = 11;
      joint.add(dot);
      guides.push(dot);
    }
  };
  let framingScale = 1;
  const getView = (): BoxView => ({
    azimuth: THREE.MathUtils.radToDeg(controls.getAzimuthalAngle()),
    elevation: 90 - THREE.MathUtils.radToDeg(controls.getPolarAngle()),
  });
  const reset = (): void => {
    if (disposed) return;
    controls.target.set(0, poseTarget, 0);
    controls.maxDistance = 9 * framingScale * poseScale;
    camera.position.set(3.6, 2.4, 5.3);
    const view = options.view ?? DEFAULT_BOX_VIEW;
    camera.position.setFromSphericalCoords(
      camera.position.length(),
      THREE.MathUtils.degToRad(90 - view.elevation),
      THREE.MathUtils.degToRad(view.azimuth),
    );
    camera.position
      .multiplyScalar(
        framingScale * poseScale * (options.collection ? 0.64 : 1),
      )
      .add(controls.target);
    controls.update();
    draw();
  };
  controls.addEventListener("change", draw);
  const resize = new ResizeObserver(() => {
    if (disposed) return;
    const { width, height } = host.getBoundingClientRect();
    surface.setSize(width, height);
    camera.aspect = width / Math.max(height, 1);
    const nextScale = Math.max(1, 1.35 / camera.aspect);
    camera.position
      .sub(controls.target)
      .multiplyScalar(nextScale / framingScale)
      .add(controls.target);
    framingScale = nextScale;
    controls.minDistance = 2.3 * framingScale;
    controls.maxDistance = 9 * framingScale * poseScale;
    camera.updateProjectionMatrix();
    draw();
  });
  resize.observe(host);
  reset();
  options.signal?.addEventListener("abort", dispose, { once: true });
  try {
    // Each box is a copy of one prepared model that shares its geometry.
    const model = cloneSkinned(await preparedModel(url));
    if (disposed) {
      disposeObject(model);
      options.signal?.throwIfAborted();
      throw new Error("Deck box was disposed while loading.");
    }
    scene.add(model);
    body = model.getObjectByName("Case");
    flap = model.getObjectByName("Flap_Assembly");
    if (!body || !flap) {
      throw new Error(
        "The inspection model is missing its case, flap or fold rig.",
      );
    }
    leather = createLeatherBend(flap);
    // Face art that arrives after the first paint needs a fresh frame.
    textures = await createBoxTextures(
      model,
      options.artworkUrl,
      options.signal,
      draw,
    );
    options.signal?.throwIfAborted();
    if (options.appearance) textures.repaint(options.appearance);
    flap.add(rear, front);
    addJointGuide(rear, 0xe4b66c);
    addJointGuide(front, 0x83d9e4);
    setGuides(false);
    pose();
    draw();
  } catch (error) {
    if (disposed) textures?.dispose();
    dispose();
    throw error;
  }
  function dispose(): void {
    if (disposed) return;
    disposed = true;
    options.signal?.removeEventListener("abort", dispose);
    cancelAnimationFrame(frame);
    cancelAnimationFrame(animationFrame);
    canvas.removeEventListener("pointermove", pointerMove);
    canvas.removeEventListener("pointerleave", pointerLeave);
    document.removeEventListener("visibilitychange", visibilityChange);
    reducedMotion.removeEventListener("change", transition);
    hitBox.geometry.dispose();
    hitBox.material.dispose();
    resize.disconnect();
    controls.dispose();
    disposeObject(scene);
    textures?.dispose();
    key.shadow.dispose();
    surface.dispose();
  }
  return {
    getView,
    setAppearance,
    setOpen,
    setInspection,
    setSeparated,
    setFold,
    setGuides,
    reset,
    dispose,
  };
}

/**
 * Each model is loaded and prepared once per page, and every box is a copy.
 * Shadows and culling are set here, so no copy has to measure its skinned
 * geometry: skin deformation can reach outside the closed model's bounds, so
 * skinned meshes are never culled. A failed load is tried again next time.
 */
const models = new Map<string, Promise<THREE.Object3D>>();
function preparedModel(url: string): Promise<THREE.Object3D> {
  let model = models.get(url);
  if (!model) {
    model = new GLTFLoader().loadAsync(url).then((gltf) => {
      gltf.scene.updateMatrixWorld(true);
      gltf.scene.traverse((object) => {
        if (!(object instanceof THREE.Mesh)) return;
        object.castShadow = true;
        object.receiveShadow = true;
        if (object instanceof THREE.SkinnedMesh) {
          object.frustumCulled = false;
          // Drawing order needs a bounding sphere; measured once at rest,
          // copies inherit it instead of skinning every vertex themselves.
          object.computeBoundingSphere();
        }
        // Shared by every copy, so no single box may release it.
        object.geometry.userData.sharedModel = true;
      });
      return gltf.scene;
    });
    model.catch(() => models.delete(url));
    models.set(url, model);
  }
  return model;
}

/** Release a box's scene. Geometry shared with other copies of its model stays. */
function disposeObject(root: THREE.Object3D): void {
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    if (!object.geometry.userData.sharedModel) object.geometry.dispose();
    if (object instanceof THREE.SkinnedMesh) object.skeleton.dispose();
    for (const material of Array.isArray(object.material)
      ? object.material
      : [object.material])
      material.dispose();
  });
}
