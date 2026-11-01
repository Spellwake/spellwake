import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";

export type BoxMotion = {
  setSeparated: (separated: boolean) => void;
  setFold: (joint: "rear" | "front", degrees: number) => void;
  setGuides: (visible: boolean) => void;
  reset: () => void;
  dispose: () => void;
};
/** Two-part construction inspection. No hover animation; render on interaction. */
export async function mountBoxMotion(
  host: HTMLElement,
  url: string,
): Promise<BoxMotion> {
  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true,
    preserveDrawingBuffer: true,
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 0.9;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  renderer.domElement.setAttribute(
    "aria-label",
    "3D deck box; drag to rotate and scroll to zoom",
  );
  renderer.domElement.setAttribute("role", "img");
  host.append(renderer.domElement);
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(30, 1, 0.01, 100);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enablePan = false;
  controls.minDistance = 2.3;
  controls.maxDistance = 9;
  controls.maxPolarAngle = Math.PI * 0.49;
  const pmrem = new THREE.PMREMGenerator(renderer);
  const room = new RoomEnvironment();
  const environment = pmrem.fromScene(room);
  scene.environment = environment.texture;
  scene.environmentIntensity = 0.45;
  room.dispose();
  pmrem.dispose();
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
  let rear: THREE.Object3D | undefined;
  let front: THREE.Object3D | undefined;
  let separated = true;
  let rearDegrees = 0;
  let frontDegrees = 0;
  const guides: THREE.Object3D[] = [];
  let frame = 0;
  const draw = (): void => {
    if (disposed || frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (!disposed) renderer.render(scene, camera);
    });
  };
  const pose = (): void => {
    if (body) body.position.x = separated ? -1.1 : 0;
    if (flap) flap.position.x = separated ? 1.1 : 0;
    if (rear) rear.rotation.x = THREE.MathUtils.degToRad(-rearDegrees);
    if (front) front.rotation.x = THREE.MathUtils.degToRad(-frontDegrees);
    host.dataset.separated = String(separated);
    host.dataset.rearFold = String(rearDegrees);
    host.dataset.frontFold = String(frontDegrees);
    draw();
  };
  const setSeparated = (value: boolean): void => {
    separated = value;
    pose();
  };
  const setFold = (joint: "rear" | "front", degrees: number): void => {
    if (joint === "rear") rearDegrees = degrees;
    else frontDegrees = degrees;
    pose();
  };
  const setGuides = (visible: boolean): void => {
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
  const reset = (): void => {
    camera.position.set(3.6, 2.6, 5.3).multiplyScalar(framingScale);
    controls.target.set(0, 0.2, 0);
    controls.update();
    draw();
  };
  controls.addEventListener("change", draw);
  const resize = new ResizeObserver(() => {
    const { width, height } = host.getBoundingClientRect();
    renderer.setSize(width, height);
    camera.aspect = width / Math.max(height, 1);
    const nextScale = Math.max(1, 1.35 / camera.aspect);
    camera.position
      .sub(controls.target)
      .multiplyScalar(nextScale / framingScale)
      .add(controls.target);
    framingScale = nextScale;
    controls.minDistance = 2.3 * framingScale;
    controls.maxDistance = 9 * framingScale;
    camera.updateProjectionMatrix();
    draw();
  });
  resize.observe(host);
  reset();
  try {
    const gltf = await new GLTFLoader().loadAsync(url);
    if (disposed) {
      gltf.scene.traverse((o) => {
        if (o instanceof THREE.Mesh) o.geometry.dispose();
      });
      return { setSeparated, setFold, setGuides, reset, dispose };
    }
    gltf.scene.traverse((o) => {
      if (o instanceof THREE.Mesh) {
        o.castShadow = true;
        o.receiveShadow = true;
      }
    });
    scene.add(gltf.scene);
    body = gltf.scene.getObjectByName("Case");
    flap = gltf.scene.getObjectByName("Flap_Assembly");
    rear = gltf.scene.getObjectByName("Rear_Fold");
    front = gltf.scene.getObjectByName("Front_Fold");
    if (!body || !flap || !rear || !front) {
      throw new Error(
        "The inspection model is missing its case, flap or fold rig.",
      );
    }
    addJointGuide(rear, 0xe4b66c);
    addJointGuide(front, 0x83d9e4);
    // Skin deformation can extend outside the closed geometry's cached bounds.
    gltf.scene.traverse((object) => {
      if (object instanceof THREE.SkinnedMesh) object.frustumCulled = false;
    });
    pose();
    draw();
  } catch (error) {
    dispose();
    throw error;
  }
  function dispose(): void {
    if (disposed) return;
    disposed = true;
    cancelAnimationFrame(frame);
    resize.disconnect();
    controls.dispose();
    scene.traverse((o) => {
      if (o instanceof THREE.Mesh) {
        o.geometry.dispose();
        for (const m of Array.isArray(o.material) ? o.material : [o.material])
          m.dispose();
      }
    });
    key.shadow.dispose();
    environment.dispose();
    renderer.dispose();
    renderer.domElement.remove();
  }
  return { setSeparated, setFold, setGuides, reset, dispose };
}
