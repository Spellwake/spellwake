import * as THREE from "three";
import { PACK, PackSheet } from "./pack-geometry";
import { foilMaterial, light } from "./pack-model";
import { printPack, type PackPrint } from "./pack-print";

/**
 * Still images of sealed packs for the library, rendered from the same
 * model, foil and studio as the opening, so the pack on the shelf is the
 * pack that lifts off it. One offscreen renderer serves every still in
 * turn and is released once the queue has been quiet for a moment.
 */

type Studio = {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  camera: THREE.OrthographicCamera;
  environment: THREE.Texture;
};

let studio: Studio | null = null;
let queue: Promise<unknown> = Promise.resolve();
let pending = 0;
let release = 0;
const RELEASE_AFTER_MS = 4000;

function open(): Studio {
  if (studio) return studio;
  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true,
    preserveDrawingBuffer: true,
  });
  renderer.setPixelRatio(1);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.NeutralToneMapping;
  renderer.toneMappingExposure = 1.05;
  renderer.setClearColor(0x000000, 0);
  const scene = new THREE.Scene();
  const environment = light(scene, renderer);
  // Straight on and edge to edge: the image is exactly the wrapper.
  const camera = new THREE.OrthographicCamera(
    -PACK.width / 2,
    PACK.width / 2,
    PACK.height / 2,
    -PACK.height / 2,
    0.1,
    10,
  );
  camera.position.z = 5;
  studio = { renderer, scene, camera, environment };
  return studio;
}

function close(): void {
  if (!studio || pending > 0) return;
  studio.environment.dispose();
  studio.renderer.dispose();
  studio.renderer.forceContextLoss();
  studio = null;
}

async function draw(print: PackPrint, width: number): Promise<ImageBitmap> {
  const { renderer, scene, camera } = open();
  const height = Math.round((width * PACK.height) / PACK.width);
  renderer.setSize(width, height, false);
  const printed = await printPack(print, {
    anisotropy: Math.min(8, renderer.capabilities.getMaxAnisotropy()),
    detail: "reduced",
    back: false,
  });
  const material = foilMaterial(printed.front);
  const sheets = (["body", "strip"] as const).flatMap((region) =>
    ([1, -1] as const).map((side) => new PackSheet(side, region)),
  );
  const pack = new THREE.Group();
  for (const sheet of sheets)
    pack.add(new THREE.Mesh(sheet.geometry, material));
  scene.add(pack);
  try {
    renderer.render(scene, camera);
    // Pixels straight from the render, never encoded as an image file.
    return await createImageBitmap(renderer.domElement);
  } finally {
    scene.remove(pack);
    for (const sheet of sheets) sheet.dispose();
    material.dispose();
    printed.dispose();
  }
}

/**
 * Render a sealed pack, face on, `width` pixels wide (its height follows
 * the wrapper's proportions), for drawing into a page canvas.
 */
export function packStill(
  print: PackPrint,
  width: number,
): Promise<ImageBitmap> {
  pending += 1;
  window.clearTimeout(release);
  const result = queue
    .catch(() => undefined)
    // Yield between stills so a full library never holds the main thread.
    .then(() => new Promise((done) => setTimeout(done, 0)))
    .then(() => draw(print, width))
    .finally(() => {
      pending -= 1;
      if (pending === 0) release = window.setTimeout(close, RELEASE_AFTER_MS);
    });
  queue = result;
  return result;
}
