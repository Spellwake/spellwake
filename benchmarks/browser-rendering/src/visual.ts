/** Matched art-direction study, independent of the measured workload. */
import * as THREE from "three";

import { z } from "zod";
const parameters = new URLSearchParams(location.search);
const engine = z
  .enum(["three", "bevy"])
  .parse(parameters.get("engine") ?? "three");
const canvas = z
  .instanceof(HTMLCanvasElement)
  .parse(document.querySelector("#table"));
const status = z
  .instanceof(HTMLOutputElement)
  .parse(document.querySelector("#status"));
const state = {
  time: 1.6,
  foil: true,
  lifted: false,
  paused:
    parameters.get("freeze") === "1" ||
    matchMedia("(prefers-reduced-motion: reduce)").matches,
  pointer: [0, 0],
  width: 1280,
  height: 720,
};
declare global {
  interface Window {
    visualStudy: boolean;
    visualState: typeof state;
  }
}
window.visualStudy = true;
window.visualState = state;
document.querySelector(`#${engine}`)?.setAttribute("aria-current", "page");
for (const name of ["foil", "lift", "pause"] as const) {
  const button = z
    .instanceof(HTMLButtonElement)
    .parse(document.querySelector(`#${name}`));
  const key = name === "lift" ? "lifted" : name === "pause" ? "paused" : "foil";
  button.setAttribute("aria-pressed", String(state[key]));
  if (name === "pause") button.textContent = state.paused ? "Resume" : "Pause";
  button.addEventListener("click", () => {
    state[key] = !state[key];
    button.setAttribute("aria-pressed", String(state[key]));
    if (name === "pause")
      button.textContent = state.paused ? "Resume" : "Pause";
  });
}
canvas.addEventListener("pointermove", (event) => {
  const rect = canvas.getBoundingClientRect();
  state.pointer = [
    ((event.clientX - rect.left) / rect.width) * 2 - 1,
    ((event.clientY - rect.top) / rect.height) * 2 - 1,
  ];
});
canvas.addEventListener("pointerleave", () => {
  state.pointer = [0, 0];
});
let previous = performance.now();
function clock(now: number): void {
  if (!state.paused) state.time += Math.min((now - previous) / 1000, 0.05);
  previous = now;
  requestAnimationFrame(clock);
}
requestAnimationFrame(clock);
const errors: string[] = [];
window.addEventListener("error", (event) => {
  errors.push(event.message);
  status.textContent = `Render error: ${event.message}`;
});
window.addEventListener(
  "unhandledrejection",
  (event: PromiseRejectionEvent) => {
    console.error(event.reason);
    status.textContent = "Renderer failed; reload to retry.";
  },
);
window.addEventListener("visual-ready", () => {
  if (!errors.length)
    status.textContent = `${engine === "bevy" ? "Bevy / Rust → Wasm" : "Three.js / TypeScript"} · WebGL2`;
});
function size(): void {
  const rect = canvas.parentElement!.getBoundingClientRect();
  state.width = Math.round(rect.width);
  state.height = Math.round(rect.height);
}
size();
new ResizeObserver(size).observe(canvas.parentElement!);

/** Same spectral approximation is implemented in public/visual/foil.wesl. */
const foilFragment = `varying vec2 vUv;uniform vec4 settings;
void main(){float t=settings.x;vec2 uv=vec2(vUv.x,1.0-vUv.y);
if(settings.w>0.5){float r=length((uv-0.5)*2.0);float ring=exp(-pow((r-0.72)*45.0,2.0));float halo=exp(-pow((r-0.72)*8.0,2.0))*0.13;float spokes=pow(max(0.0,sin(atan(uv.y-.5,uv.x-.5)*12.0-t*.8)),18.0)*ring;gl_FragColor=vec4(vec3(.2,.85,1.)*(ring+halo+spokes),min(.8,ring*.6+halo)*settings.y);return;}
float sweep=sin(uv.x*6.0+uv.y*3.5-t*.65+settings.z*1.5)*.5+.5;
vec3 spectral=.5+.5*cos(6.2831853*(sweep+vec3(0.,.33,.67)));
float band=pow(max(0.0,sin((uv.x*.8+uv.y*.55-t*.09+settings.z*.1)*6.2831853)),12.0);
float fleck=pow(max(0.0,sin(uv.x*730.0+uv.y*190.0)*sin(uv.y*640.0-uv.x*240.0)),12.0);
float border=step(.045,uv.x)*step(uv.x,.955)*step(.035,uv.y)*step(uv.y,.965);
gl_FragColor=vec4(spectral*(.45+fleck*.6),(.08+band*.38+fleck*.08)*border*settings.y);}`;
async function startThree(): Promise<void> {
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  renderer.setPixelRatio(1);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.15;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x101f23);
  const camera = new THREE.PerspectiveCamera(42, 1, 0.1, 100);
  scene.add(new THREE.HemisphereLight(0xb9dce0, 0x343028, 2));
  const key = new THREE.DirectionalLight(0xffe3b0, 3.2);
  key.position.set(-4, 9, 5);
  key.castShadow = true;
  key.shadow.mapSize.set(2048, 2048);
  key.shadow.camera.left = -10;
  key.shadow.camera.right = 10;
  key.shadow.camera.top = 10;
  key.shadow.camera.bottom = -10;
  key.shadow.normalBias = 0.025;
  scene.add(key);
  const rim = new THREE.DirectionalLight(0x66cfe6, 1.6);
  rim.position.set(5, 5, -5);
  scene.add(rim);
  const loader = new THREE.TextureLoader();
  const [art, table, shadow, ...dice] = await Promise.all(
    ["astral-hart.png", "table.png", "shadow.png", "die-atlas.png"].map(
      async (name) => {
        const texture = await loader.loadAsync(`/visual/${name}`);
        texture.colorSpace = THREE.SRGBColorSpace;
        texture.anisotropy = 8;
        return texture;
      },
    ),
  );
  if (!art || !table || !shadow) throw new Error("Missing shared textures");
  const meshSchema = z.object({
    positions: z.array(z.number()),
    normals: z.array(z.number()),
    uvs: z.array(z.number()),
    indices: z.array(z.number().int().nonnegative()).nullable(),
  });
  const source = z
    .object({ base: meshSchema, card: meshSchema, die: meshSchema })
    .parse(await (await fetch("/visual/models.json")).json());
  function geometry(data: z.infer<typeof meshSchema>): THREE.BufferGeometry {
    const g = new THREE.BufferGeometry();
    g.setAttribute(
      "position",
      new THREE.Float32BufferAttribute(data.positions, 3),
    );
    g.setAttribute("normal", new THREE.Float32BufferAttribute(data.normals, 3));
    g.setAttribute("uv", new THREE.Float32BufferAttribute(data.uvs, 2));
    if (data.indices) g.setIndex(data.indices);
    return g;
  }
  const models = {
    base: geometry(source.base),
    card: geometry(source.card),
    die: geometry(source.die),
  };
  const mat = (
    color: number,
    metalness = 0,
    roughness = 0.65,
  ): THREE.MeshStandardMaterial =>
    new THREE.MeshStandardMaterial({ color, metalness, roughness });
  function mesh(
    geometry: THREE.BufferGeometry,
    material: THREE.Material | THREE.Material[],
    x: number,
    y: number,
    z: number,
  ): THREE.Mesh {
    const object = new THREE.Mesh(geometry, material);
    object.position.set(x, y, z);
    object.castShadow = true;
    object.receiveShadow = true;
    scene.add(object);
    return object;
  }
  mesh(models.base, mat(0x41362b, 0.6, 0.45), 0, -0.35, 0);
  mesh(
    new THREE.BoxGeometry(15.7, 0.1, 11.7),
    new THREE.MeshStandardMaterial({
      map: table,
      roughness: 0.82,
      metalness: 0.12,
    }),
    0,
    -0.025,
    0,
  );
  const cardGeometry = models.card;
  const frontGeometry = new THREE.PlaneGeometry(1.86, 2.81);
  frontGeometry.rotateX(-Math.PI / 2);
  const cardMaterial = new THREE.MeshStandardMaterial({
    map: art,
    roughness: 0.48,
    metalness: 0.08,
  });
  const baseMaterial = mat(0x151d20, 0.15, 0.35);
  const cards: THREE.Group[] = [];
  const positions = [
    [-3.7, 0.04, -1.5, -0.12],
    [-1.5, 0.04, -1.5, 0.05],
    [-3.5, 0.04, 1.7, 0.12],
    [-1.3, 0.04, 1.8, -0.08],
    [2, 0.8, 0.25, -0.12],
    [4.6, 0.04, -2.5, 0.08],
  ];
  for (const pose of positions) {
    const [x = 0, y = 0, z = 0, angle = 0] = pose;
    const group = new THREE.Group();
    group.position.set(x, y, z);
    group.rotation.y = angle;
    const body = new THREE.Mesh(cardGeometry, baseMaterial);
    body.castShadow = true;
    body.receiveShadow = true;
    group.add(body);
    const face = new THREE.Mesh(frontGeometry, cardMaterial);
    face.position.y = 0.037;
    face.receiveShadow = false;
    group.add(face);
    scene.add(group);
    cards.push(group);
  }
  const hero = cards[4];
  if (!hero) throw new Error("Missing hero card");
  const foilMaterial = new THREE.ShaderMaterial({
    vertexShader:
      "varying vec2 vUv;void main(){vUv=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);}",
    fragmentShader: foilFragment,
    uniforms: { settings: { value: new THREE.Vector4(0, 1, 0, 0) } },
    transparent: true,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
  });
  const foil = new THREE.Mesh(frontGeometry, foilMaterial);
  foil.position.y = 0.04;
  hero.add(foil);
  for (let i = 0; i < 5; i++) {
    const group = new THREE.Group();
    group.position.set(
      -1.3 + i * 0.62,
      0.09 + i * 0.013,
      4.1 + Math.abs(i - 2) * 0.08,
    );
    group.rotation.y = (i - 2) * -0.11;
    group.add(new THREE.Mesh(cardGeometry, baseMaterial));
    const face = new THREE.Mesh(frontGeometry, cardMaterial);
    face.position.y = 0.037;
    group.add(face);
    scene.add(group);
  }
  for (let i = 0; i < 8; i++) {
    const back = mesh(
      cardGeometry,
      mat(0x1e3c40, 0.35, 0.35),
      5.3,
      0.07 + i * 0.075,
      1.8,
    );
    back.rotation.y = 0.14;
  }
  const dieMaterials = new THREE.MeshStandardMaterial({
    map: dice[0] ?? null,
    roughness: 0.32,
    metalness: 0.12,
  });
  const die1 = mesh(models.die, dieMaterials, 4, 0.42, 0.1);
  die1.rotation.set(0.1, 0.4, 0.1);
  const die2 = mesh(models.die, dieMaterials, 4.9, 0.46, 0.45);
  die2.rotation.set(0.15, -0.4, -0.12);
  const spellMaterial = foilMaterial.clone();
  spellMaterial.uniforms = {
    settings: { value: new THREE.Vector4(0, 1, 0, 1) },
  };
  const spell = new THREE.Mesh(
    new THREE.PlaneGeometry(5.5, 5.5),
    spellMaterial,
  );
  spell.rotation.x = -Math.PI / 2;
  spell.position.set(2, 0.04, 0.25);
  scene.add(spell);
  const contact = new THREE.Mesh(
    new THREE.PlaneGeometry(3.4, 4.2),
    new THREE.MeshBasicMaterial({
      map: shadow,
      transparent: true,
      depthWrite: false,
    }),
  );
  contact.rotation.x = -Math.PI / 2;
  contact.position.set(2, 0.045, 0.25);
  scene.add(contact);
  const particles: THREE.Mesh[] = [];
  const glow = new THREE.MeshBasicMaterial({ color: 0x8cdbe9 });
  for (let i = 0; i < 28; i++)
    particles.push(mesh(new THREE.SphereGeometry(0.022, 6, 4), glow, 0, 0, 0));
  let lift = 0;
  let oldWidth = 0,
    oldHeight = 0;
  function draw(): void {
    if (oldWidth !== state.width || oldHeight !== state.height) {
      renderer.setSize(state.width, state.height, false);
      oldWidth = state.width;
      oldHeight = state.height;
    }
    camera.aspect = state.width / state.height;
    const narrow = camera.aspect < 1;
    camera.position.set(0, narrow ? 18 : 10, narrow ? 15 : 11);
    camera.lookAt(0, 0, 0.3);
    camera.updateProjectionMatrix();
    const t = state.time;
    lift += (Number(state.lifted) - lift) * 0.1;
    const px = state.pointer[0] ?? 0;
    const py = state.pointer[1] ?? 0;
    hero!.position.y = 0.8 + lift * 1.1;
    hero!.scale.setScalar(1 + lift * 0.5);
    hero!.rotation.set(
      -0.12 + lift * 0.45 + py * 0.13,
      -0.12 + px * 0.15,
      0.045 - px * 0.07,
    );
    foilMaterial.uniforms.settings!.value.set(t, Number(state.foil), px, 0);
    spellMaterial.uniforms.settings!.value.set(t, 1, 0, 1);
    spell.rotation.z = t * 0.08;
    for (const [i, p] of particles.entries()) {
      const a = i * 2.399 + t * 0.2;
      const r = 2.1 + Math.sin(i) * 0.25;
      p.position.set(
        2 + Math.sin(a) * r,
        0.12 + ((i * 0.13 + t * 0.14) % 1.2),
        0.25 + Math.cos(a) * r,
      );
      p.scale.setScalar(0.6 + Math.sin(i + t) * 0.25);
    }
    renderer.render(scene, camera);
    requestAnimationFrame(draw);
  }
  draw();
  window.dispatchEvent(new Event("visual-ready"));
}

if (engine === "bevy") {
  const url = "/wasm/bevy.js";
  const wasm = await import(/* @vite-ignore */ url);
  await wasm.default();
} else {
  await startThree();
}
