import * as THREE from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";

/**
 * A box's own visible canvas. The page's one WebGL renderer draws the box's
 * scene and the result is copied here, so every box shares one context, one
 * lighting environment and one set of compiled shader programs.
 */
export type BoxSurface = {
  readonly canvas: HTMLCanvasElement;
  /** Size in CSS pixels; the drawing buffer follows the surface's pixel ratio. */
  setSize: (width: number, height: number) => void;
  /** Draw the scene now and show it. The scene's environment is the shared room. */
  render: (scene: THREE.Scene, camera: THREE.Camera) => void;
  dispose: () => void;
};

type Surface = {
  width: number;
  height: number;
  /** Draws the box again after the shared context has been restored. */
  redraw: () => void;
};

type SharedRenderer = {
  renderer: THREE.WebGLRenderer;
  environment: THREE.WebGLRenderTarget;
  surfaces: Set<Surface>;
  width: number;
  height: number;
  fitted: boolean;
};

let shared: SharedRenderer | undefined;

function roomEnvironment(
  renderer: THREE.WebGLRenderer,
): THREE.WebGLRenderTarget {
  const pmrem = new THREE.PMREMGenerator(renderer);
  const room = new RoomEnvironment();
  const environment = pmrem.fromScene(room);
  room.dispose();
  pmrem.dispose();
  return environment;
}

/**
 * The renderer lives for the page: it holds no box's resources, and keeping
 * it spares the next arrival a new context and environment. A failed
 * creation throws to the box and is tried again by the next one.
 */
function sharedRenderer(): SharedRenderer {
  if (shared) return shared;
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  // Surfaces have their own ratios, so this canvas is sized in device pixels.
  renderer.setPixelRatio(1);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 0.9;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  const state: SharedRenderer = {
    renderer,
    environment: roomEnvironment(renderer),
    surfaces: new Set(),
    width: 0,
    height: 0,
    fitted: false,
  };
  // Three.js rebuilds its own state first; the environment's pixels were lost
  // with the context, so it is captured again and every box redrawn.
  renderer.domElement.addEventListener("webglcontextrestored", () => {
    state.environment = roomEnvironment(renderer);
    state.width = state.height = 0;
    state.fitted = false;
    for (const surface of state.surfaces) surface.redraw();
  });
  shared = state;
  return state;
}

/** The shared canvas is as large as the largest live surface, and no larger. */
function fit(state: SharedRenderer): void {
  if (state.fitted) return;
  let width = 1;
  let height = 1;
  for (const surface of state.surfaces) {
    width = Math.max(width, surface.width);
    height = Math.max(height, surface.height);
  }
  if (width !== state.width || height !== state.height) {
    state.renderer.setSize(width, height, false);
    state.width = width;
    state.height = height;
  }
  state.fitted = true;
}

/**
 * Create a box's visible canvas, drawn by the shared renderer.
 *
 * @throws When WebGL or a 2D canvas context is unavailable.
 */
export function createBoxSurface(
  pixelRatio: number,
  redraw: () => void,
): BoxSurface {
  const state = sharedRenderer();
  const canvas = document.createElement("canvas");
  canvas.style.display = "block";
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas drawing is unavailable.");
  const surface: Surface = { width: 0, height: 0, redraw };
  state.surfaces.add(surface);
  let disposed = false;
  return {
    canvas,
    setSize(width, height) {
      if (disposed) return;
      canvas.width = Math.floor(width * pixelRatio);
      canvas.height = Math.floor(height * pixelRatio);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      surface.width = canvas.width;
      surface.height = canvas.height;
      state.fitted = false;
    },
    render(scene, camera) {
      const { renderer } = state;
      if (disposed || !canvas.width || !canvas.height) return;
      // A lost context draws nothing; the box keeps its last picture until restored.
      if (renderer.getContext().isContextLost()) return;
      fit(state);
      scene.environment = state.environment.texture;
      // The box is drawn in the shared canvas's lower left, its own size.
      renderer.setViewport(0, 0, canvas.width, canvas.height);
      renderer.render(scene, camera);
      // Copied in the same task, before the drawing buffer can be presented.
      context.clearRect(0, 0, canvas.width, canvas.height);
      context.drawImage(
        renderer.domElement,
        0,
        state.height - canvas.height,
        canvas.width,
        canvas.height,
        0,
        0,
        canvas.width,
        canvas.height,
      );
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      state.surfaces.delete(surface);
      state.fitted = false;
      // Release a large surface's share of the buffer now, not at the next draw.
      if (!state.renderer.getContext().isContextLost()) fit(state);
      canvas.remove();
    },
  };
}
