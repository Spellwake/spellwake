import * as THREE from "three";
import { z } from "zod";

const cardSchema = z.object({
  x: z.number(),
  y: z.number(),
  z: z.number(),
  angle: z.number(),
  art: z.number().int().min(0).max(63),
});
const sceneSchema = z.object({
  version: z.literal(1),
  width: z.literal(1280),
  height: z.literal(720),
  cardWidth: z.number(),
  cardHeight: z.number(),
  cards: z.array(cardSchema).min(6000),
});
type Card = z.infer<typeof cardSchema>;
declare global {
  interface Window {
    benchmarkScene: unknown;
    benchmarkTime: number;
  }
}
document.documentElement.style.setProperty(
  "--device-scale",
  String(devicePixelRatio),
);
const parameters = new URLSearchParams(location.search);
const engine = z
  .enum(["three-objects", "three-instanced", "bevy"])
  .parse(parameters.get("engine") ?? "three-instanced");
const count = z.coerce
  .number()
  .int()
  .min(1)
  .max(6000)
  .parse(parameters.get("count") ?? 120);
const trial = z.coerce
  .number()
  .int()
  .min(0)
  .max(20)
  .parse(parameters.get("trial") ?? 0);
const batch = parameters.get("batch") ?? `manual-${Date.now()}`;
const freeze = parameters.get("freeze") === "1";
const status = document.querySelector("#status");
const output = document.querySelector("#result");
const canvas = z
  .instanceof(HTMLCanvasElement)
  .parse(document.querySelector("canvas"));
if (!status || !output) throw new Error("Missing benchmark fixture elements");
status.textContent = `${engine} · ${count} cards · WebGL2 · ${freeze ? "frozen parity view" : `trial ${trial + 1}`}`;
const scene = sceneSchema.parse(await (await fetch("/scene.json")).json());
window.benchmarkScene = { ...scene, count };
window.benchmarkTime = 0;
const engineIntervals: number[] = [];
let lastEngineTick: number | null = null;
let drawCalls = 0;
let preflightDrawCalls = 0;
let countersRestored = false;
const intervals: number[] = [];
const cpu: number[] = [];
let readyAt: number | null = null;
let previous: number | null = null;
let done = false;
let hidden = false;
let backendVerified = false;
const errors: string[] = [];
let rendererInfo: Record<string, unknown> | null = null;
const navigationStarted = performance.now();
const instrumented = new WeakSet<WebGL2RenderingContext>();
const restoreCounters: Array<() => void> = [];
function instrument(context: WebGL2RenderingContext): void {
  if (instrumented.has(context)) return;
  instrumented.add(context);
  for (const method of [
    "drawArrays",
    "drawElements",
    "drawArraysInstanced",
    "drawElementsInstanced",
  ] as const) {
    const original = context[method];
    const descriptor = Object.getOwnPropertyDescriptor(context, method);
    restoreCounters.push(() => {
      if (descriptor) Object.defineProperty(context, method, descriptor);
      else Reflect.deleteProperty(context, method);
    });
    Object.defineProperty(context, method, {
      configurable: true,
      value: new Proxy(original, {
        apply(target, receiver, args) {
          drawCalls++;
          return Reflect.apply(target, receiver, args);
        },
      }),
    });
  }
}
window.addEventListener("engine-tick", () => {
  const now = performance.now();
  if (readyAt !== null && now - readyAt > 3000 && !freeze) {
    if (lastEngineTick !== null) engineIntervals.push(now - lastEngineTick);
    else preflightDrawCalls = drawCalls;
    lastEngineTick = now;
  }
});
// Both backends create WebGL2 on this fixture canvas. Instrument its actual draws
// before engine initialization, without supplying or changing context attributes.
const originalGetContext = canvas.getContext.bind(canvas);
Object.defineProperty(canvas, "getContext", {
  value: (...args: Parameters<HTMLCanvasElement["getContext"]>) => {
    const context = Reflect.apply(originalGetContext, canvas, args);
    if (context instanceof WebGL2RenderingContext) instrument(context);
    return context;
  },
});
window.addEventListener("renderer-ready", () => {
  const context = canvas.getContext("webgl2");
  if (!context) throw new Error("Expected a real WebGL2 context");
  const debug = context.getExtension("WEBGL_debug_renderer_info");
  rendererInfo = {
    ...rendererInfo,
    vendor: debug
      ? context.getParameter(debug.UNMASKED_VENDOR_WEBGL)
      : context.getParameter(context.VENDOR),
    renderer: debug
      ? context.getParameter(debug.UNMASKED_RENDERER_WEBGL)
      : context.getParameter(context.RENDERER),
  };
  backendVerified = true;
  readyAt = performance.now();
});
document.addEventListener("visibilitychange", () => {
  if (document.hidden) hidden = true;
});
window.addEventListener("error", (event) => {
  errors.push(event.message);
  status!.textContent = `Benchmark error: ${event.message}`;
  void finish();
});
window.addEventListener("unhandledrejection", (event) => {
  errors.push(String(event.reason));
  status!.textContent = `Benchmark error: ${String(event.reason)}`;
  void finish();
});
canvas.addEventListener("webglcontextlost", () => {
  errors.push("webgl-context-lost");
  void finish();
});
let render: ((t: number) => void) | undefined;
function quantile(values: number[], fraction: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return (
    sorted[
      Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * fraction))
    ] ?? 0
  );
}
async function finish(): Promise<void> {
  if (done) return;
  done = true;
  const invalidReasons = [...errors];
  if (hidden || document.hidden)
    invalidReasons.push("not-continuously-visible");
  if (!backendVerified) invalidReasons.push("backend-not-verified");
  if (engineIntervals.length < 30)
    invalidReasons.push("insufficient-engine-ticks");
  if (preflightDrawCalls < 30)
    invalidReasons.push("insufficient-preflight-draws");
  if (!countersRestored) invalidReasons.push("draw-counters-not-removed");
  if (canvas.width !== 1280 || canvas.height !== 720)
    invalidReasons.push("wrong-drawing-buffer-size");
  if (
    /swiftshader|llvmpipe|softpipe|software/i.test(
      String(rendererInfo?.["renderer"] ?? ""),
    )
  )
    invalidReasons.push("software-renderer");
  const result = {
    schemaVersion: 1,
    valid: invalidReasons.length === 0,
    invalidReasons,
    backendVerified,
    index: parameters.has("index") ? Number(parameters.get("index")) : null,
    batch,
    engine,
    count,
    trial,
    backend: "webgl2",
    width: 1280,
    height: 720,
    actualWidth: canvas.width,
    actualHeight: canvas.height,
    scene: "unlit-tabletop-v1",
    frozen: false,
    visibleThroughout: !hidden && !document.hidden,
    sceneLoadedToReadyMilliseconds:
      readyAt === null ? null : readyAt - navigationStarted,
    engineSamples: engineIntervals.length,
    preflightDrawCalls,
    drawInstrumentationDuringTiming: false,
    engineTickMilliseconds: {
      p50: quantile(engineIntervals, 0.5),
      p95: quantile(engineIntervals, 0.95),
      p99: quantile(engineIntervals, 0.99),
    },
    samples: intervals.length,
    frameMilliseconds: {
      p50: quantile(intervals, 0.5),
      p95: quantile(intervals, 0.95),
      p99: quantile(intervals, 0.99),
    },
    framesOver33Milliseconds: intervals.filter((v) => v > 33.34).length,
    rendererCpuMilliseconds: cpu.length
      ? { p50: quantile(cpu, 0.5), p95: quantile(cpu, 0.95) }
      : null,
    engineIntervals,
    frameIntervals: intervals,
    userAgent: navigator.userAgent,
    devicePixelRatio: devicePixelRatio,
    hardwareConcurrency: navigator.hardwareConcurrency,
    rendererInfo,
    limitations: [
      "Vsync-capped observer RAF and engine-tick intervals, not presentation FPS or isolated GPU time",
      "Unlit synthetic atlas, no shadows or physics",
      "Bevy CPU/GPU durations and total GPU memory not instrumented; engine tick is a progress signal",
      "No mobile or battery result",
    ],
  };
  output!.textContent = JSON.stringify(result, null, 2);
  status!.textContent = `${result.valid ? "Completed" : "Invalid"} ${engine} · ${count} cards · ${result.frameMilliseconds.p95.toFixed(2)} ms p95`;
  const response = await fetch("/results", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(result),
  });
  if (!response.ok) throw new Error("Could not persist benchmark result");
  const envelope = z
    .object({
      status: z.literal("success"),
      data: z.object({ next: z.string().nullable() }),
    })
    .parse(await response.json());
  if (parameters.get("suite") === "1" && envelope.data.next)
    location.assign(envelope.data.next);
}
function frame(now: number): void {
  if (done) return;
  if (readyAt !== null) {
    window.benchmarkTime = freeze ? 1 : (now - readyAt) / 1000;
    const measuring = now - readyAt > 3000;
    if (measuring && !countersRestored) {
      preflightDrawCalls = drawCalls;
      for (const restore of restoreCounters) restore();
      countersRestored = true;
    }
    if (measuring && !freeze && previous !== null)
      intervals.push(now - previous);
    previous = now;
    if (render) {
      const before = performance.now();
      render(window.benchmarkTime);
      if (measuring && !freeze) cpu.push(performance.now() - before);
    }
    if (!freeze && now - readyAt > 13000) {
      void finish().catch((error) => {
        status!.textContent = String(error);
      });
      return;
    }
  }
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

if (engine === "bevy") {
  const wasmPath = "/wasm/bevy.js";
  const module = z
    .object({
      default: z.custom<() => Promise<unknown>>(
        (value) => typeof value === "function",
      ),
    })
    .parse(await import(wasmPath));
  await module.default();
} else {
  const world = new THREE.Scene();
  world.background = new THREE.Color(0x111111);
  const camera = new THREE.PerspectiveCamera(45, 1280 / 720, 0.1, 100);
  camera.position.set(0, 16, 15);
  camera.lookAt(0, 0, 0);
  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: false,
    alpha: false,
    powerPreference: "high-performance",
  });
  renderer.setPixelRatio(1);
  renderer.setSize(1280, 720, false);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.NoToneMapping;
  const gl = renderer.getContext();
  const debug = gl.getExtension("WEBGL_debug_renderer_info");
  rendererInfo = {
    vendor: debug
      ? gl.getParameter(debug.UNMASKED_VENDOR_WEBGL)
      : gl.getParameter(gl.VENDOR),
    renderer: debug
      ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL)
      : gl.getParameter(gl.RENDERER),
  };
  const atlas = await new THREE.TextureLoader().loadAsync("/atlas.png");
  atlas.colorSpace = THREE.SRGBColorSpace;
  atlas.magFilter = THREE.NearestFilter;
  atlas.minFilter = THREE.NearestFilter;
  atlas.generateMipmaps = false;
  atlas.flipY = false;
  const material = new THREE.MeshBasicMaterial({
    map: atlas,
    side: THREE.DoubleSide,
  });
  const geometries = Array.from({ length: 64 }, (_, art) => {
    const geometry = new THREE.PlaneGeometry(scene.cardWidth, scene.cardHeight);
    const uv = geometry.getAttribute("uv");
    for (let i = 0; i < uv.count; i++)
      uv.setXY(
        i,
        (uv.getX(i) + (art % 8)) / 8,
        (1 - uv.getY(i) + Math.floor(art / 8)) / 8,
      );
    return geometry;
  });
  const cards = scene.cards.slice(0, count);
  const dummy = new THREE.Object3D();
  const horizontal = new THREE.Quaternion().setFromAxisAngle(
    new THREE.Vector3(1, 0, 0),
    -Math.PI / 2,
  );
  const vertical = new THREE.Vector3(0, 1, 0);
  const objects: THREE.Mesh[] = [];
  const batches: { mesh: THREE.InstancedMesh; indices: number[] }[] = [];
  if (engine === "three-objects")
    for (const card of cards) {
      const geometry = geometries[card.art];
      if (!geometry) throw new Error("Missing art");
      const mesh = new THREE.Mesh(geometry, material);
      world.add(mesh);
      objects.push(mesh);
    }
  else
    for (let art = 0; art < 64; art++) {
      const indices = cards.flatMap((c, i) => (c.art === art ? [i] : []));
      if (!indices.length) continue;
      const geometry = geometries[art];
      if (!geometry) throw new Error("Missing geometry");
      const mesh = new THREE.InstancedMesh(geometry, material, indices.length);
      mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
      mesh.frustumCulled = false;
      world.add(mesh);
      batches.push({ mesh, indices });
    }
  const table = new THREE.Mesh(
    new THREE.BoxGeometry(20, 0.2, 20),
    new THREE.MeshBasicMaterial({ color: 0x25352f }),
  );
  table.position.y = -0.05;
  world.add(table);
  const dice = Array.from({ length: 6 }, (_, i) => {
    const die = new THREE.Mesh(
      new THREE.BoxGeometry(0.55, 0.55, 0.55),
      new THREE.MeshBasicMaterial({ color: 0xdcd4be }),
    );
    const a = (i * Math.PI) / 3;
    die.position.set(Math.sin(a) * 2, 1, Math.cos(a) * 2);
    world.add(die);
    return die;
  });
  const effectGeometry = new THREE.PlaneGeometry(0.18, 0.18);
  const effectMaterial = new THREE.MeshBasicMaterial({
    color: new THREE.Color().setRGB(0.25, 0.75, 1, THREE.SRGBColorSpace),
    opacity: 0.45,
    transparent: true,
    depthWrite: false,
    forceSinglePass: true,
    side: THREE.DoubleSide,
  });
  const effects = Array.from({ length: 64 }, () => {
    const mesh = new THREE.Mesh(effectGeometry, effectMaterial);
    world.add(mesh);
    return mesh;
  });
  function pose(
    card: Card,
    i: number,
    t: number,
    target: THREE.Object3D,
  ): void {
    target.position.set(
      card.x,
      card.y + 0.03 * Math.sin(t * 1.2 + i * 0.13),
      card.z,
    );
    target.quaternion
      .setFromAxisAngle(vertical, card.angle + 0.015 * Math.sin(t + i * 0.5))
      .multiply(horizontal);
    target.updateMatrix();
  }
  render = (t: number) => {
    if (engine === "three-objects")
      objects.forEach((mesh, i) => {
        const card = cards[i];
        if (card) pose(card, i, t, mesh);
      });
    else
      for (const group of batches) {
        group.indices.forEach((i, j) => {
          const card = cards[i];
          if (card) {
            pose(card, i, t, dummy);
            group.mesh.setMatrixAt(j, dummy.matrix);
          }
        });
        group.mesh.instanceMatrix.needsUpdate = true;
      }
    dice.forEach((die, i) => {
      die.position.y = 1 + 0.2 * Math.sin(t + i);
      die.rotation.set(t * 0.7 + i, t * 0.9 + i, t * 0.4 + i);
    });
    effects.forEach((effect, i) => {
      const a = i * 0.31 + t * 0.2;
      effect.position.set(
        Math.sin(a) * 2.5,
        0.6 + Math.sin(i * 0.37 + t * 0.4) * 0.5,
        Math.cos(a) * 2.5,
      );
      effect.rotation.x = -Math.PI / 2;
    });
    renderer.render(world, camera);
    window.dispatchEvent(new Event("engine-tick"));
    rendererInfo = {
      ...rendererInfo,
      drawCalls: renderer.info.render.calls,
      triangles: renderer.info.render.triangles,
    };
  };
  render(0);
  window.dispatchEvent(new Event("renderer-ready"));
}
