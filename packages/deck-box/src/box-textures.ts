import * as THREE from "three";
import { makeFrameRamp } from "./color-blend";
import { BOX_PALETTE } from "./material-palette";
import { MANA_INK } from "./appearance";
import {
  MANA_ORDER,
  DEFAULT_APPEARANCE,
  type BoxAppearance,
} from "./box-appearance";
const WIDTH = 1.6;
const LENGTH = 3.558407044966673;
const SIZE = 1024;
const SYMBOL_CODES = ["W", "U", "B", "R", "G", "C"] as const;
type SymbolCode = (typeof SYMBOL_CODES)[number];
/**
 * The host serves the box's texture inputs from one public folder, beside
 * the model: the leather maps in `materials/`, the official mana symbols as
 * Scryfall serves them in `mana-symbols/`, and the Spellwake mark as
 * `logo.svg`. A release build has no other copy of them.
 */
const artwork = (root: string, path: string): string =>
  new URL(path, new URL(root, document.baseURI)).href;

/**
 * The official symbol's glyph without its coloured disc, inked in the
 * identity colour. The disc is the symbol's first shape; the glyph's own
 * paths are left exactly as drawn.
 */
async function glyphImage(
  artworkUrl: string,
  code: SymbolCode,
): Promise<HTMLImageElement> {
  const response = await fetch(artwork(artworkUrl, `mana-symbols/${code}.svg`));
  if (!response.ok) throw new Error(`Unable to load the {${code}} symbol.`);
  const svg = new DOMParser().parseFromString(
    await response.text(),
    "image/svg+xml",
  ).documentElement;
  const shapes = [...svg.querySelectorAll("circle, path")];
  shapes[0]?.remove();
  for (const shape of shapes.slice(1))
    shape.setAttribute("fill", MANA_INK[code]);
  svg.setAttribute("width", "512");
  svg.setAttribute("height", "512");
  const url = URL.createObjectURL(
    new Blob([new XMLSerializer().serializeToString(svg)], {
      type: "image/svg+xml",
    }),
  );
  try {
    return await image(url);
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Soft, tileable abrasion for the leather, drawn in code: layered value
 * noise, brighter where the surface has been rubbed. Shared by every box.
 */
let wearCanvas: HTMLCanvasElement | null = null;
function wearPattern(): HTMLCanvasElement {
  if (wearCanvas) return wearCanvas;
  const size = 512;
  let seed = 0x9e3779b9;
  const random = (): number => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 2 ** 32;
  };
  const octaves = [4, 8, 16, 32, 64, 128].map((cells, index) => ({
    cells,
    weight: 0.55 ** index,
    values: Float32Array.from({ length: cells * cells }, random),
  }));
  const total = octaves.reduce((sum, octave) => sum + octave.weight, 0);
  const ease = (t: number): number => t * t * (3 - 2 * t);
  const context = surface(size);
  context.canvas.width = size;
  const pixels = context.createImageData(size, size);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      let value = 0;
      for (const { cells, weight, values } of octaves) {
        const fx = (x / size) * cells;
        const fy = (y / size) * cells;
        const x0 = Math.floor(fx) % cells;
        const y0 = Math.floor(fy) % cells;
        const x1 = (x0 + 1) % cells;
        const y1 = (y0 + 1) % cells;
        const tx = ease(fx - Math.floor(fx));
        const ty = ease(fy - Math.floor(fy));
        const at = (cx: number, cy: number): number =>
          values[cy * cells + cx] ?? 0;
        const top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        const bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        value += (top + (bottom - top) * ty) * weight;
      }
      // Mostly settled leather, with rubbed patches rising out of it.
      const level = Math.min(1, Math.max(0, (value / total - 0.32) * 1.6));
      const index = (y * size + x) * 4;
      const tone = Math.round((0.22 + level * 0.42) * 255);
      pixels.data[index] = tone;
      pixels.data[index + 1] = tone;
      pixels.data[index + 2] = tone;
      pixels.data[index + 3] = 255;
    }
  }
  context.putImageData(pixels, 0, 0);
  wearCanvas = context.canvas;
  return wearCanvas;
}
function surface(height = SIZE): CanvasRenderingContext2D {
  const canvas = document.createElement("canvas");
  canvas.width = SIZE;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas textures are unavailable");
  return context;
}
async function image(url: string): Promise<HTMLImageElement> {
  const result = new Image();
  const isLocalBlobOrData = url.startsWith("blob:") || url.startsWith("data:");
  // Card art may come from Scryfall's servers; it must not taint the texture canvas.
  if (!isLocalBlobOrData) {
    result.crossOrigin = "anonymous";
  }
  result.src = url;
  try {
    await result.decode();
  } catch (cause) {
    if (isLocalBlobOrData) {
      throw new Error(`Unable to decode local box texture: ${url.slice(0, 32)}`, {
        cause,
      });
    }
    console.warn(`Retrying box texture after decode failure: ${url}`, cause);
    const retryUrl = new URL(url);
    retryUrl.searchParams.set("retry", "1");
    result.src = retryUrl.href;
    try {
      await result.decode();
    } catch (retryCause) {
      throw new Error(`Unable to decode box texture: ${url}`, {
        cause: retryCause,
      });
    }
  }
  return result;
}
type Glyph = {
  image: HTMLImageElement;
  x: number;
  y: number;
  width: number;
  height: number;
};
function trimGlyph(source: HTMLImageElement): Glyph {
  const ctx = surface();
  ctx.canvas.width = source.naturalWidth;
  ctx.canvas.height = source.naturalHeight;
  ctx.drawImage(source, 0, 0);
  const { width, height } = ctx.canvas;
  const pixels = ctx.getImageData(0, 0, width, height).data;
  let left = width,
    right = 0,
    top = height,
    bottom = 0;
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++) {
      if ((pixels[(y * width + x) * 4 + 3] ?? 0) < 32) continue;
      left = Math.min(left, x);
      right = Math.max(right, x);
      top = Math.min(top, y);
      bottom = Math.max(bottom, y);
    }
  return {
    image: source,
    x: left,
    y: top,
    width: right - left + 1,
    height: bottom - top + 1,
  };
}
/** UVs stay in the flat leather's material space, so the printed image bends with it. */
function mapMesh(mesh: THREE.Mesh, cover: boolean): void {
  const geometry = mesh.geometry;
  // Copies of one prepared model share its geometry, which is mapped once.
  if (geometry.userData.leatherMapped) return;
  geometry.computeVertexNormals();
  const position = geometry.getAttribute("position");
  const normal = geometry.getAttribute("normal");
  const uv = new Float32Array(position.count * 2);
  const grainUv = new Float32Array(position.count * 2);
  for (let i = 0; i < position.count; i++) {
    const x = position.getX(i),
      y = position.getY(i),
      z = position.getZ(i);
    uv[i * 2] = (x + WIDTH / 2) / WIDTH;
    uv[i * 2 + 1] = cover ? 1 - y / LENGTH : (y + 0.96) / 1.92;
    const side = !cover && Math.abs(normal.getX(i)) > 0.7;
    grainUv[i * 2] = (side ? z : x) * 2;
    grainUv[i * 2 + 1] = y * 2;
  }
  geometry.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
  geometry.setAttribute("uv1", new THREE.BufferAttribute(grainUv, 2));
  const index = geometry.getIndex();
  if (!index) throw new Error("Expected indexed leather geometry");
  const outer: number[] = [],
    lining: number[] = [];
  for (let i = 0; i < index.count; i += 3) {
    const a = index.getX(i),
      b = index.getX(i + 1),
      c = index.getX(i + 2);
    const z = (position.getZ(a) + position.getZ(b) + position.getZ(c)) / 3;
    const nz = (normal.getZ(a) + normal.getZ(b) + normal.getZ(c)) / 3;
    const printed = cover ? z < 0.01 : z > 0.4 && nz > 0.45;
    (printed ? outer : lining).push(a, b, c);
  }
  geometry.setIndex([...outer, ...lining]);
  geometry.clearGroups();
  geometry.addGroup(0, outer.length, 0);
  geometry.addGroup(outer.length, lining.length, 1);
  geometry.userData.leatherMapped = true;
}
type SharedInputs = {
  normal: HTMLImageElement;
  roughness: HTMLImageElement;
  glyphs: Map<string, Glyph>;
  /** The leather photograph as a neutral grain, for tinting and wear. */
  grain: CanvasRenderingContext2D;
  /** Where printed ink thins with the wear. */
  inkMask: CanvasRenderingContext2D;
  /** The Spellwake mark (the approved SVG), stamped on a box with no face card. */
  emblem: HTMLImageElement;
};
let shared: Promise<SharedInputs> | null = null;
/**
 * Everything boxes have in common: the leather maps, the official mana
 * glyphs and the painted grain and ink mask. Reading and painting them is
 * the costly part of a box, so it happens once per page; each box only
 * makes its own textures from them. A failed read is tried again next time.
 */
function sharedInputs(artworkUrl: string): Promise<SharedInputs> {
  shared ??= (async () => {
    const material = (name: string) =>
      image(artwork(artworkUrl, `materials/${name}`));
    const [leather, normal, roughness, glyphEntries, emblem] =
      await Promise.all([
        material("leather-color.jpg"),
        material("leather-normal.jpg"),
        material("leather-roughness.jpg"),
        Promise.all(
          SYMBOL_CODES.map(async (code): Promise<[string, Glyph]> => [
            code,
            trimGlyph(await glyphImage(artworkUrl, code)),
          ]),
        ),
        image(artwork(artworkUrl, "logo.svg")),
      ]);
    const wear = wearPattern();
    const inkMask = surface();
    inkMask.drawImage(wear, 0, 0, SIZE, SIZE);
    const inkPixels = inkMask.getImageData(0, 0, SIZE, SIZE);
    for (let i = 0; i < inkPixels.data.length; i += 4) {
      const abrasion = Math.max(
        0,
        Math.min(1, ((inkPixels.data[i] ?? 0) / 255 - 0.2) / 0.4),
      );
      // Ink thins on the same rubbed grain; the substrate supplies the revealed color.
      inkPixels.data[i + 3] = Math.round(255 * (0.94 - abrasion * 0.3));
    }
    inkMask.putImageData(inkPixels, 0, 0);
    const grain = surface();
    grain.drawImage(leather, 0, 0, SIZE, SIZE);
    const grainPixels = grain.getImageData(0, 0, SIZE, SIZE);
    for (let i = 0; i < grainPixels.data.length; i += 4) {
      const value =
        178 +
        ((grainPixels.data[i] ?? 0) * 0.3 +
          (grainPixels.data[i + 1] ?? 0) * 0.6 +
          (grainPixels.data[i + 2] ?? 0) * 0.1) *
          0.5;
      grainPixels.data[i] = value;
      grainPixels.data[i + 1] = value;
      grainPixels.data[i + 2] = value;
    }
    grain.putImageData(grainPixels, 0, 0);
    return {
      normal,
      roughness,
      glyphs: new Map(glyphEntries),
      grain,
      inkMask,
      emblem,
    };
  })().catch((error: unknown) => {
    shared = null;
    throw error;
  });
  return shared;
}
type SharedMaps = {
  normalMap: THREE.Texture;
  roughnessMap: THREE.Texture;
  wearMap: THREE.Texture;
  grainMap: THREE.Texture;
};
let leatherMaps: SharedMaps | null = null;
/**
 * The leather maps no box paints, made once so the page's one renderer
 * uploads them once. They live for the page and no box releases them.
 */
function sharedMaps(inputs: SharedInputs): SharedMaps {
  if (leatherMaps) return leatherMaps;
  const wearMap = new THREE.CanvasTexture(wearPattern());
  const imageTexture = (source: HTMLImageElement): THREE.Texture => {
    const texture = new THREE.Texture(source);
    texture.needsUpdate = true;
    return texture;
  };
  const normalMap = imageTexture(inputs.normal);
  const roughnessMap = imageTexture(inputs.roughness);
  for (const texture of [normalMap, roughnessMap, wearMap]) {
    texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
    texture.channel = 1;
    texture.anisotropy = 4;
  }
  const grainMap = new THREE.CanvasTexture(inputs.grain.canvas);
  grainMap.wrapS = grainMap.wrapT = THREE.RepeatWrapping;
  grainMap.anisotropy = 4;
  leatherMaps = { normalMap, roughnessMap, wearMap, grainMap };
  return leatherMaps;
}
export type BoxTextures = {
  repaint: (appearance: BoxAppearance) => void;
  dispose: () => void;
};
/**
 * Apply the SVG study's identity ramp, the face card's art and the official
 * mana symbols to the approved 3D anatomy. `onRepaint` is called when face
 * art that was still loading has been printed, so the caller can redraw.
 */
export async function createBoxTextures(
  root: THREE.Object3D,
  artworkUrl: string,
  signal?: AbortSignal,
  onRepaint?: () => void,
): Promise<BoxTextures> {
  signal?.throwIfAborted();
  // The images and painted surfaces every box shares are prepared once per page.
  const inputs = await sharedInputs(artworkUrl);
  signal?.throwIfAborted();
  const glyphs = inputs.glyphs;
  const grain = inputs.grain;
  const inkMask = inputs.inkMask;
  const { normalMap, roughnessMap, wearMap, grainMap } = sharedMaps(inputs);
  const front = surface(),
    wrap = surface(2048),
    plain = surface(),
    liner = surface(2048);
  const maps = [front, wrap, plain, liner].map((ctx) => {
    const texture = new THREE.CanvasTexture(ctx.canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.anisotropy = 4;
    return texture;
  });
  const material = (
    map: THREE.Texture | undefined,
    lining = false,
    cover = false,
    detail = false,
    preserveBand = false,
  ): THREE.MeshStandardMaterial => {
    const result = new THREE.MeshStandardMaterial({
      ...(map ? { map } : {}),
      color: lining ? 0xaca7a0 : 0xffffff,
      normalMap,
      normalScale: new THREE.Vector2(
        lining ? 0.42 : 0.28,
        lining ? 0.42 : 0.28,
      ),
      roughnessMap,
      roughness: 0.93,
      metalness: 0.04,
    });
    result.onBeforeCompile = (shader) => {
      shader.uniforms.wearMap = { value: wearMap };
      shader.uniforms.grainMap = { value: grainMap };
      shader.uniforms.surfaceDetail = { value: detail ? 1 : 0 };
      shader.uniforms.preserveBand = { value: preserveBand ? 1 : 0 };
      shader.uniforms.printedCover = { value: cover && !lining ? 1 : 0 };
      shader.uniforms.coverSurface = { value: cover ? 1 : 0 };
      shader.uniforms.wearStrength = { value: lining ? 0.55 : 1 };
      shader.vertexShader =
        "varying vec2 vWearUv;\nvarying vec3 vWearPosition;\n" +
        shader.vertexShader;
      shader.vertexShader = shader.vertexShader.replace(
        "#include <uv_vertex>",
        "#include <uv_vertex>\nvWearUv = uv1 * 0.4;\nvWearPosition = position;",
      );
      shader.fragmentShader =
        "uniform sampler2D wearMap;\nuniform sampler2D grainMap;\nuniform float surfaceDetail;\nuniform float preserveBand;\nuniform float printedCover;\nuniform float coverSurface;\nuniform float wearStrength;\nvarying vec2 vWearUv;\nvarying vec3 vWearPosition;\n" +
        shader.fragmentShader;
      // Broad wear modulates the same identity tone instead of covering it with gray.
      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <map_fragment>",
        `#include <map_fragment>
        float xEdge = abs(abs(vWearPosition.x) - mix(0.78, 0.74, coverSurface));
        float caseY = min(abs(vWearPosition.y + 0.94), abs(vWearPosition.y - 0.90));
        float caseZ = abs(abs(vWearPosition.z) - 0.51);
        float foldEnd = min(min(abs(vWearPosition.y - 0.04), abs(vWearPosition.y - 1.18)),
                            min(abs(vWearPosition.y - 2.378), abs(vWearPosition.y - 3.52)));
        float cornerDistance = mix(length(vec3(xEdge, caseY, caseZ)), length(vec2(xEdge, foldEnd)), coverSurface);
        float corner = 1.0 - smoothstep(0.025, 0.24, cornerDistance);
        float caseEdge = min(length(vec2(xEdge, caseY)), min(length(vec2(xEdge, caseZ)), length(vec2(caseY, caseZ))));
        float edge = 1.0 - smoothstep(0.012, 0.07, mix(caseEdge, xEdge, coverSurface));
        float detailMask = surfaceDetail * (1.0 - printedCover * smoothstep(1.95, 2.174, vWearPosition.y));
        detailMask *= mix(1.0, smoothstep(-0.44, -0.34, vWearPosition.y), preserveBand);
        float wearMask = (mix(0.12, 0.28, detailMask) + corner * 0.78 + edge * 0.10) * wearStrength;
        float wear = mix(0.5, clamp(0.08 + texture2D(wearMap, vWearUv).r * 1.2, 0.18, 0.78), wearMask);
        float leatherGrain = texture2D(grainMap, vWearUv * 1.6).r;
        diffuseColor.rgb *= 0.82 * mix(1.0, 0.50 + leatherGrain * 0.65, detailMask);
        // Soft pooling between rubbed seams gives every unprinted face material depth.
        float panelShade = mix(1.0, 0.84 + 0.16 * (1.0 - edge), detailMask);
        diffuseColor.rgb *= panelShade * mix(1.0, 0.78, detailMask * preserveBand);
        diffuseColor.rgb = wear <= 0.5 ? diffuseColor.rgb * wear * 2.0
          : mix(diffuseColor.rgb, vec3(1.0), (wear - 0.5) * 0.8);
      `,
      );
      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <roughnessmap_fragment>",
        `#include <roughnessmap_fragment>
        roughnessFactor *= mix(1.0, 0.68, smoothstep(0.4, 0.7, wear));
      `,
      );
    };
    result.customProgramCacheKey = () => "complete-leather-v4";
    return result;
  };
  const frontMaterial = material(maps[0], false, false, true, true),
    wrapMaterial = material(maps[1], false, true, true);
  const plainMaterial = material(maps[2], false, false, true),
    liningMaterial = material(maps[3], true, true, true);
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    if (object.name !== "Case" && object.name !== "Flap") return;
    for (const old of Array.isArray(object.material)
      ? object.material
      : [object.material])
      old.dispose();
    const cover = object.name === "Flap";
    mapMesh(object, cover);
    object.material = cover
      ? [wrapMaterial, liningMaterial]
      : [frontMaterial, plainMaterial];
  });
  // Face art, by URL, once loaded. A repaint asked for while art is loading
  // prints plain leather now and the art when it arrives.
  const faces = new Map<string, Promise<HTMLImageElement | null>>();
  const loaded = new Map<string, HTMLImageElement | null>();
  let latest: BoxAppearance = DEFAULT_APPEARANCE;
  let disposed = false;
  function repaint(appearance: BoxAppearance): void {
    latest = appearance;
    const { faceArt } = appearance;
    if (faceArt && !loaded.has(faceArt) && !faces.has(faceArt)) {
      faces.set(
        faceArt,
        image(faceArt)
          .catch((error: unknown) => {
            console.warn("Showing plain leather without the face card.", error);
            return null;
          })
          .then((art) => {
            loaded.set(faceArt, art);
            if (!disposed && latest.faceArt === faceArt) {
              paint(latest);
              onRepaint?.();
            }
            return art;
          }),
      );
    }
    paint(appearance);
  }
  function paint({ colors, faceArt }: BoxAppearance): void {
    const ordered = MANA_ORDER.filter((color) => colors.includes(color));
    const codes =
      ordered.length >= 3 ? ["gold"] : ordered.length ? ordered : ["C"];
    const ramp = makeFrameRamp(
      codes.map((code) => BOX_PALETTE[code] ?? BOX_PALETTE.C ?? "#899197"),
    );
    for (const ctx of [front, wrap, plain, liner]) {
      ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
      for (let x = 0; x < SIZE; x++) {
        const color = ramp[x] ?? [0, 0, 0];
        ctx.fillStyle = `rgb(${color.map((channel) => Math.round(channel * 255)).join(" ")})`;
        ctx.fillRect(x, 0, 1, ctx.canvas.height);
      }
      ctx.save();
      ctx.globalCompositeOperation = "multiply";
      const pattern = ctx.createPattern(grain.canvas, "repeat");
      if (pattern) {
        pattern.setTransform(new DOMMatrix().scale(0.5));
        ctx.fillStyle = pattern;
        ctx.fillRect(0, 0, SIZE, ctx.canvas.height);
      }
      ctx.restore();
    }
    // Unprinted surfaces use the same shaded leather family as the symbol band.
    for (const ctx of [plain, liner]) {
      const depth = ctx.createLinearGradient(0, 0, 0, ctx.canvas.height);
      depth.addColorStop(0, "rgba(0,0,0,.32)");
      depth.addColorStop(0.2, "rgba(0,0,0,.10)");
      depth.addColorStop(0.7, "rgba(0,0,0,.18)");
      depth.addColorStop(1, "rgba(0,0,0,.38)");
      ctx.fillStyle = depth;
      ctx.fillRect(0, 0, SIZE, ctx.canvas.height);
    }
    const source = faceArt ? loaded.get(faceArt) : null;
    if (source) {
      const x = 0,
        width = SIZE;
      const y = (2.174 / LENGTH) * 2048,
        height = ((LENGTH - 2.174) / LENGTH) * 2048;
      const print = surface(2048);
      print.save();
      print.beginPath();
      print.rect(x, y, width, height);
      print.clip();
      print.filter = "saturate(0.86) contrast(0.92) brightness(1.02)";
      const ratio = WIDTH / (LENGTH - 2.174);
      const cropWidth = Math.min(
        source.naturalWidth,
        source.naturalHeight * ratio,
      );
      const cropHeight = cropWidth / ratio;
      print.drawImage(
        source,
        (source.naturalWidth - cropWidth) / 2,
        (source.naturalHeight - cropHeight) / 2,
        cropWidth,
        cropHeight,
        x,
        y,
        width,
        height,
      );
      print.filter = "none";
      print.globalCompositeOperation = "multiply";
      print.globalAlpha = 0.38;
      print.drawImage(grain.canvas, x, y, width, height);
      print.globalAlpha = 1;
      print.globalCompositeOperation = "destination-in";
      print.drawImage(inkMask.canvas, x, y, width, height);
      const fade = print.createLinearGradient(0, y, 0, y + height);
      fade.addColorStop(0, "rgba(0,0,0,0)");
      fade.addColorStop(0.08, "rgba(0,0,0,.12)");
      fade.addColorStop(0.28, "rgba(0,0,0,.68)");
      fade.addColorStop(1, "rgba(0,0,0,.68)");
      print.fillStyle = fade;
      print.fillRect(x, y, width, height);
      print.restore();
      wrap.save();
      wrap.globalCompositeOperation = "multiply";
      wrap.globalAlpha = 0.22;
      wrap.drawImage(print.canvas, 0, 0);
      wrap.restore();
      wrap.drawImage(print.canvas, 0, 0);
    } else {
      // A deck with no cards has no face card yet; it carries the Spellwake
      // mark, stamped in its own gold and worn with the leather.
      const flapTop = (2.174 / LENGTH) * 2048;
      const flapHeight = ((LENGTH - 2.174) / LENGTH) * 2048;
      const height = flapHeight * 0.4;
      const width =
        (height * inputs.emblem.naturalWidth) / inputs.emblem.naturalHeight;
      const left = (SIZE - width) / 2;
      const top = flapTop + flapHeight * 0.58 - height / 2;
      const mark = surface(2048);
      mark.drawImage(inputs.emblem, left, top, width, height);
      mark.globalCompositeOperation = "destination-in";
      mark.drawImage(inkMask.canvas, 0, flapTop, SIZE, flapHeight);
      wrap.save();
      wrap.shadowColor = "rgba(0,0,0,.6)";
      wrap.shadowBlur = 10;
      wrap.shadowOffsetY = 4;
      wrap.drawImage(mark.canvas, 0, 0);
      wrap.restore();
    }
    // Burnished seams and a recessed symbol band belong to material, not the art crop.
    for (const ctx of [front, wrap]) {
      const edge = ctx.createLinearGradient(0, 0, SIZE, 0);
      edge.addColorStop(0, "rgba(0,0,0,.70)");
      edge.addColorStop(0.055, "rgba(0,0,0,.34)");
      edge.addColorStop(0.085, "rgba(0,0,0,0)");
      edge.addColorStop(0.915, "rgba(0,0,0,0)");
      edge.addColorStop(0.945, "rgba(0,0,0,.34)");
      edge.addColorStop(1, "rgba(0,0,0,.70)");
      ctx.fillStyle = edge;
      ctx.fillRect(0, 0, SIZE, ctx.canvas.height);
    }
    const band = front.createLinearGradient(0, 720, 0, 1024);
    band.addColorStop(0, "rgba(0,0,0,.55)");
    band.addColorStop(0.18, "rgba(0,0,0,.22)");
    band.addColorStop(0.65, "rgba(0,0,0,.35)");
    band.addColorStop(1, "rgba(0,0,0,.60)");
    front.fillStyle = band;
    front.fillRect(0, 720, SIZE, 304);
    const symbols = (ordered.length ? ordered : ["C"])
      .map((code) => glyphs.get(code))
      .filter((glyph): glyph is Glyph => glyph !== undefined);
    const symbolHeight = 112,
      gap = 44;
    // Center within the usable band between the flap and the bottom stitched seam.
    const bandTop = ((0.96 + 0.4) / 1.92) * SIZE;
    const bandBottom = ((0.96 + 0.9) / 1.92) * SIZE;
    const symbolTop = (bandTop + bandBottom - symbolHeight) / 2;
    const widths = symbols.map(
      (glyph) => (symbolHeight * glyph.width) / glyph.height,
    );
    let left =
      (SIZE -
        widths.reduce((sum, width) => sum + width, 0) -
        gap * (symbols.length - 1)) /
      2;
    for (const [i, glyph] of symbols.entries()) {
      const width = widths[i] ?? symbolHeight;
      front.save();
      front.shadowColor = "rgba(0,0,0,.65)";
      front.shadowBlur = 3;
      front.shadowOffsetY = 2;
      front.drawImage(
        glyph.image,
        glyph.x,
        glyph.y,
        glyph.width,
        glyph.height,
        left,
        symbolTop,
        width,
        symbolHeight,
      );
      front.restore();
      left += width + gap;
    }
    for (const map of maps) map.needsUpdate = true;
  }
  repaint(DEFAULT_APPEARANCE);
  return {
    repaint,
    dispose: () => {
      disposed = true;
      for (const map of maps) map.dispose();
    },
  };
}
