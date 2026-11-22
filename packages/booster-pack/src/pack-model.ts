import * as THREE from "three";
import type { SidePrint } from "./pack-print";

/**
 * What the opening stage and the library stills share: the studio the foil
 * mirrors, its lights and the foil material itself, so a pack on the shelf
 * and a pack being opened are the same object.
 */

/** Parse "#rrggbb" or "#rrggbbaa" into a colour and its alpha. */
export function colour(hex: string): { color: THREE.Color; alpha: number } {
  const value = hex.replace("#", "");
  const color = new THREE.Color(`#${value.slice(0, 6)}`);
  const alpha = value.length >= 8 ? parseInt(value.slice(6, 8), 16) / 255 : 1;
  return { color, alpha };
}

/**
 * A dark studio with long softboxes, so metallic foil has something to
 * mirror: a warm key above, cool strips at the sides, a rim behind and a
 * dim fill where the camera stands.
 */
function studio(renderer: THREE.WebGLRenderer): THREE.Texture {
  const scene = new THREE.Scene();
  const room = new THREE.Mesh(
    new THREE.BoxGeometry(30, 30, 30),
    new THREE.MeshBasicMaterial({ color: 0x07090d, side: THREE.BackSide }),
  );
  scene.add(room);
  const panel = (
    width: number,
    height: number,
    position: [number, number, number],
    color: number,
    intensity: number,
  ): void => {
    const mesh = new THREE.Mesh(
      new THREE.PlaneGeometry(width, height),
      new THREE.MeshBasicMaterial({
        color: new THREE.Color(color).multiplyScalar(intensity),
        side: THREE.DoubleSide,
      }),
    );
    mesh.position.set(...position);
    mesh.lookAt(0, 0, 0);
    scene.add(mesh);
  };
  panel(9, 3.2, [-3.5, 7, 7], 0xfff1dc, 7);
  panel(1.2, 14, [9, 0.5, 3], 0xd6e6ff, 4.2);
  panel(1, 14, [-9, -0.5, 1], 0xffd9a6, 3.2);
  panel(12, 1.6, [0, 8, -7], 0xffffff, 6);
  // Behind the camera: what the face of the pack mirrors.
  panel(11, 4.5, [0, 3, 11], 0xfff4e6, 2.1);
  panel(7, 3, [0, -3.5, 11], 0xb8c4d8, 0.8);
  panel(20, 20, [0, -11, 0], 0x1d1712, 1);
  const pmrem = new THREE.PMREMGenerator(renderer);
  const target = pmrem.fromScene(scene, 0.035);
  pmrem.dispose();
  scene.traverse((object) => {
    if (object instanceof THREE.Mesh) {
      object.geometry.dispose();
      (object.material as THREE.Material).dispose();
    }
  });
  return target.texture;
}

/** Light a scene with the studio and its key, rim and fill. Returns the environment to dispose. */
export function light(
  scene: THREE.Scene,
  renderer: THREE.WebGLRenderer,
): THREE.Texture {
  const environment = studio(renderer);
  scene.environment = environment;
  scene.add(new THREE.HemisphereLight(0xdfe8ff, 0x2a2018, 0.35));
  const key = new THREE.DirectionalLight(0xffe6c4, 1.6);
  key.position.set(-2.5, 4, 5);
  scene.add(key);
  const rim = new THREE.DirectionalLight(0xbcd2ff, 1.4);
  rim.position.set(3, 2, -4);
  scene.add(rim);
  return environment;
}

/** Printed foil: tinted lacquer over bright metal, a clear coat, holographic where printed so. */
export function foilMaterial(maps: SidePrint): THREE.MeshPhysicalMaterial {
  const material = new THREE.MeshPhysicalMaterial({
    map: maps.color,
    normalMap: maps.normal,
    normalScale: new THREE.Vector2(0.85, 0.85),
    roughnessMap: maps.finish,
    metalnessMap: maps.finish,
    iridescenceMap: maps.finish,
    roughness: 1,
    metalness: 1,
    iridescence: 1,
    iridescenceIOR: 1.38,
    iridescenceThicknessRange: [160, 640],
    clearcoat: 0.55,
    clearcoatRoughness: 0.12,
    envMapIntensity: 1.25,
    alphaTest: 0.5,
    side: THREE.DoubleSide,
  });
  // Inside the pouch the foil is bare: unprinted, satin silver.
  material.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <map_fragment>",
      [
        "#include <map_fragment>",
        // Coloured foil is tinted lacquer over bright metal: lift the darks.
        "diffuseColor.rgb = gl_FrontFacing ? mix(diffuseColor.rgb, sqrt(diffuseColor.rgb), 0.26) : vec3(0.62, 0.6, 0.57);",
      ].join("\n"),
    );
  };
  material.customProgramCacheKey = () => "spellwake-pack-foil";
  return material;
}
