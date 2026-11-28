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
const HEIGHT = 1.92;
const LENGTH = 3.558407044966673;
const SIZE = 1024;
const SYMBOL_CODES = ["W", "U", "B", "R", "G", "C"] as const;
type SymbolCode = (typeof SYMBOL_CODES)[number];

/**
 * Per-glyph optical scale factors so airy/spiky glyphs (W), tall narrow glyphs
 * (U), and dense/chunky glyphs (B, G, R) read as equal optical size side by side.
 */
const MANA_OPTICAL_SCALE: Record<SymbolCode, number> = {
  W: 1.08,
  U: 0.94,
  B: 0.85,
  R: 0.91,
  G: 0.87,
  C: 0.98,
};

type MagnetImprint = {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
};

/**
 * Twin hidden circular rare-earth magnet coordinates (exact 1:1 3D mating pair
 * between the exposed Case front wall and the inner Flap suede lining).
 */
const FRONT_MAGNETS: readonly MagnetImprint[] = [
  {
    cx: SIZE * 0.235,
    cy: ((0.96 + 0.15) / HEIGHT) * SIZE,
    rx: 0.082 * (SIZE / WIDTH),
    ry: 0.082 * (SIZE / HEIGHT),
  },
  {
    cx: SIZE * 0.765,
    cy: ((0.96 + 0.15) / HEIGHT) * SIZE,
    rx: 0.082 * (SIZE / WIDTH),
    ry: 0.082 * (SIZE / HEIGHT),
  },
];

const LINER_MAG_Y = ((LENGTH - 0.225) / LENGTH) * 2048;
const LINER_MAGNETS: readonly MagnetImprint[] = [
  {
    cx: SIZE * 0.235,
    cy: LINER_MAG_Y,
    rx: 0.082 * (SIZE / WIDTH),
    ry: 0.082 * (2048 / LENGTH),
  },
  {
    cx: SIZE * 0.765,
    cy: LINER_MAG_Y,
    rx: 0.082 * (SIZE / WIDTH),
    ry: 0.082 * (2048 / LENGTH),
  },
];

/**
 * The host serves the box's texture inputs from one public folder, beside
 * the model: the leather maps in `materials/`, the official mana symbols as
 * Scryfall serves them in `mana-symbols/`, and the Spellwake mark as
 * `logo.svg`. A release build has no other copy of them.
 */
const artwork = (root: string, path: string): string =>
  new URL(path, new URL(root, document.baseURI)).href;

type Glyph = {
  image: HTMLImageElement;
  x: number;
  y: number;
  width: number;
  height: number;
};

type GlyphPair = {
  color: Glyph;
  mask: Glyph;
};

/**
 * Loads one official mana symbol without its colored disc, producing both an
 * identity-inked glyph image and a pure-white silhouette mask from the same SVG.
 */
async function glyphPair(
  artworkUrl: string,
  code: SymbolCode,
): Promise<GlyphPair> {
  const response = await fetch(artwork(artworkUrl, `mana-symbols/${code}.svg`));
  if (!response.ok) throw new Error(`Unable to load the {${code}} symbol.`);
  const rawSvg = await response.text();
  const buildVariant = (fill: string): string => {
    const svg = new DOMParser().parseFromString(
      rawSvg,
      "image/svg+xml",
    ).documentElement;
    const shapes = [...svg.querySelectorAll("circle, path")];
    shapes[0]?.remove();
    for (const shape of shapes.slice(1)) shape.setAttribute("fill", fill);
    svg.setAttribute("width", "512");
    svg.setAttribute("height", "512");
    return URL.createObjectURL(
      new Blob([new XMLSerializer().serializeToString(svg)], {
        type: "image/svg+xml",
      }),
    );
  };
  const colorUrl = buildVariant(MANA_INK[code]);
  const maskUrl = buildVariant("#ffffff");
  try {
    const [colorImg, maskImg] = await Promise.all([
      image(colorUrl),
      image(maskUrl),
    ]);
    const color = trimGlyph(colorImg);
    const mask: Glyph = {
      image: maskImg,
      x: color.x,
      y: color.y,
      width: color.width,
      height: color.height,
    };
    return { color, mask };
  } finally {
    URL.revokeObjectURL(colorUrl);
    URL.revokeObjectURL(maskUrl);
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
  const context = surface(size, size);
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

function surface(height = SIZE, width = SIZE): CanvasRenderingContext2D {
  const canvas = document.createElement("canvas");
  canvas.width = width;
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
      throw new Error(
        `Unable to decode local box texture: ${url.slice(0, 32)}`,
        {
          cause,
        },
      );
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

function trimGlyph(source: HTMLImageElement): Glyph {
  const ctx = surface(source.naturalHeight, source.naturalWidth);
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
    width: Math.max(1, right - left + 1),
    height: Math.max(1, bottom - top + 1),
  };
}

/**
 * Samples the Flap mesh's exact outer half-width as a function of Y so the
 * perimeter stitch track follows the real physical flap rim.
 */
function sampleFlapOuterContour(
  mesh: THREE.Mesh,
  bins = 72,
): (y: number) => number {
  const pos = mesh.geometry.getAttribute("position");
  const maxX = new Float32Array(bins + 1);
  for (let i = 0; i < pos.count; i++) {
    if (pos.getZ(i) >= 0.01) continue;
    const y = pos.getY(i);
    const ax = Math.abs(pos.getX(i));
    const bin = Math.max(0, Math.min(bins, Math.round((y / LENGTH) * bins)));
    if (ax > (maxX[bin] ?? 0)) maxX[bin] = ax;
  }
  for (let b = 1; b <= bins; b++) {
    if ((maxX[b] ?? 0) < 0.5) maxX[b] = maxX[b - 1] ?? 0.735;
  }
  for (let b = bins - 1; b >= 0; b--) {
    if ((maxX[b] ?? 0) < 0.5) maxX[b] = maxX[b + 1] ?? 0.735;
  }
  return (y: number): number => {
    const f = Math.max(0, Math.min(bins, (y / LENGTH) * bins));
    const i0 = Math.floor(f);
    const i1 = Math.min(bins, i0 + 1);
    const t = f - i0;
    return (maxX[i0] ?? 0.735) * (1 - t) + (maxX[i1] ?? 0.735) * t;
  };
}

/**
 * Computes per-vertex `aStitch` (`vec2`: x = sculpted thread crown, y = stitch
 * groove) directly from the GLB mesh's 12-step Laplacian protrusion inside the
 * real 3D stitch bands across all 360 degrees. Runs once per shared geometry.
 */
function computeStitchAttribute(
  mesh: THREE.Mesh,
  isCover: boolean,
  flapContourFn?: (y: number) => number,
): void {
  const geometry = mesh.geometry;
  if (geometry.getAttribute("aStitch")) return;
  const posAttr = geometry.getAttribute("position");
  const normAttr = geometry.getAttribute("normal");
  const idx = geometry.getIndex();
  const nV = posAttr.count;
  const stitchArr = new Float32Array(nV * 2);
  if (!idx || !normAttr) {
    geometry.setAttribute("aStitch", new THREE.BufferAttribute(stitchArr, 2));
    return;
  }

  const posArr = new Float32Array(nV * 3);
  const normArr = new Float32Array(nV * 3);
  for (let i = 0; i < nV; i++) {
    const i3 = i * 3;
    posArr[i3] = posAttr.getX(i);
    posArr[i3 + 1] = posAttr.getY(i);
    posArr[i3 + 2] = posAttr.getZ(i);
    normArr[i3] = normAttr.getX(i);
    normArr[i3 + 1] = normAttr.getY(i);
    normArr[i3 + 2] = normAttr.getZ(i);
  }

  const cur = new Float32Array(posArr);
  const nxt = new Float32Array(nV * 3);
  const deg = new Float32Array(nV);
  const maxEdge = new Float32Array(nV);

  for (let i = 0; i < idx.count; i += 3) {
    const a = idx.getX(i);
    const b = idx.getX(i + 1);
    const c = idx.getX(i + 2);
    deg[a] = (deg[a] ?? 0) + 2;
    deg[b] = (deg[b] ?? 0) + 2;
    deg[c] = (deg[c] ?? 0) + 2;
    const a3 = a * 3;
    const b3 = b * 3;
    const c3 = c * 3;
    const ax = posArr[a3] ?? 0,
      ay = posArr[a3 + 1] ?? 0,
      az = posArr[a3 + 2] ?? 0;
    const bx = posArr[b3] ?? 0,
      by = posArr[b3 + 1] ?? 0,
      bz = posArr[b3 + 2] ?? 0;
    const cx = posArr[c3] ?? 0,
      cy = posArr[c3 + 1] ?? 0,
      cz = posArr[c3 + 2] ?? 0;
    const dab = Math.hypot(ax - bx, ay - by, az - bz);
    const dbc = Math.hypot(bx - cx, by - cy, bz - cz);
    const dca = Math.hypot(cx - ax, cy - ay, cz - az);
    if (dab > (maxEdge[a] ?? 0)) maxEdge[a] = dab;
    if (dca > (maxEdge[a] ?? 0)) maxEdge[a] = dca;
    if (dab > (maxEdge[b] ?? 0)) maxEdge[b] = dab;
    if (dbc > (maxEdge[b] ?? 0)) maxEdge[b] = dbc;
    if (dbc > (maxEdge[c] ?? 0)) maxEdge[c] = dbc;
    if (dca > (maxEdge[c] ?? 0)) maxEdge[c] = dca;
  }

  for (let it = 0; it < 12; it++) {
    nxt.fill(0);
    for (let i = 0; i < idx.count; i += 3) {
      const a = idx.getX(i);
      const b = idx.getX(i + 1);
      const c = idx.getX(i + 2);
      const a3 = a * 3;
      const b3 = b * 3;
      const c3 = c * 3;
      const ax = cur[a3] ?? 0,
        ay = cur[a3 + 1] ?? 0,
        az = cur[a3 + 2] ?? 0;
      const bx = cur[b3] ?? 0,
        by = cur[b3 + 1] ?? 0,
        bz = cur[b3 + 2] ?? 0;
      const cx = cur[c3] ?? 0,
        cy = cur[c3 + 1] ?? 0,
        cz = cur[c3 + 2] ?? 0;
      nxt[a3] = (nxt[a3] ?? 0) + bx + cx;
      nxt[a3 + 1] = (nxt[a3 + 1] ?? 0) + by + cy;
      nxt[a3 + 2] = (nxt[a3 + 2] ?? 0) + bz + cz;
      nxt[b3] = (nxt[b3] ?? 0) + ax + cx;
      nxt[b3 + 1] = (nxt[b3 + 1] ?? 0) + ay + cy;
      nxt[b3 + 2] = (nxt[b3 + 2] ?? 0) + az + cz;
      nxt[c3] = (nxt[c3] ?? 0) + ax + bx;
      nxt[c3 + 1] = (nxt[c3 + 1] ?? 0) + ay + by;
      nxt[c3 + 2] = (nxt[c3 + 2] ?? 0) + az + bz;
    }
    for (let i = 0; i < nV; i++) {
      const d = deg[i] || 1;
      const i3 = i * 3;
      cur[i3] = (cur[i3] ?? 0) * 0.5 + ((nxt[i3] ?? 0) / d) * 0.5;
      cur[i3 + 1] = (cur[i3 + 1] ?? 0) * 0.5 + ((nxt[i3 + 1] ?? 0) / d) * 0.5;
      cur[i3 + 2] = (cur[i3 + 2] ?? 0) * 0.5 + ((nxt[i3 + 2] ?? 0) / d) * 0.5;
    }
  }

  const smoothstep = (e0: number, e1: number, v: number): number => {
    const t = Math.max(0, Math.min(1, (v - e0) / (e1 - e0)));
    return t * t * (3 - 2 * t);
  };

  // Measure Case outer side-wall half-width |x| by Y so tapered upper side and back stitches match 100%.
  const sideBins = 64;
  const caseMaxX = new Float32Array(sideBins + 1);
  if (!isCover) {
    for (let i = 0; i < nV; i++) {
      const i3 = i * 3;
      const ax = Math.abs(posArr[i3] ?? 0);
      const y = posArr[i3 + 1] ?? 0;
      const b = Math.max(
        0,
        Math.min(sideBins, Math.round(((y + 0.96) / HEIGHT) * sideBins)),
      );
      if (ax > (caseMaxX[b] ?? 0)) caseMaxX[b] = ax;
    }
    for (let b = 1; b <= sideBins; b++) {
      if ((caseMaxX[b] ?? 0) < 0.5) caseMaxX[b] = caseMaxX[b - 1] ?? 0.78;
    }
    for (let b = sideBins - 1; b >= 0; b--) {
      if ((caseMaxX[b] ?? 0) < 0.5) caseMaxX[b] = caseMaxX[b + 1] ?? 0.78;
    }
  }
  const caseHalfWAtY = (y: number): number => {
    const f = Math.max(0, Math.min(sideBins, ((y + 0.96) / HEIGHT) * sideBins));
    const b0 = Math.floor(f);
    const b1 = Math.min(sideBins, b0 + 1);
    const t = f - b0;
    return (caseMaxX[b0] ?? 0.78) * (1 - t) + (caseMaxX[b1] ?? 0.78) * t;
  };

  for (let i = 0; i < nV; i++) {
    // Reject vertices connected to long flat panel triangles to prevent inward interpolation bleed.
    if ((maxEdge[i] ?? 0) > 0.0115) continue;
    const i3 = i * 3;
    const x = posArr[i3] ?? 0;
    const y = posArr[i3 + 1] ?? 0;
    const z = posArr[i3 + 2] ?? 0;
    const nx = normArr[i3] ?? 0;
    const ny = normArr[i3 + 1] ?? 0;
    const nz = normArr[i3 + 2] ?? 0;
    const ax = Math.abs(x);
    const az = Math.abs(z);

    let inBand = false;
    if (isCover) {
      if (z < 0) {
        const edgeX = flapContourFn ? flapContourFn(y) : 0.735;
        const distFromSide = edgeX - ax;
        const onFrontTip = y > 3.482 && y < 3.508 && ax < edgeX - 0.048;
        const onBackTip = y > 0.048 && y < 0.076 && ax < edgeX - 0.048;
        const onSideTrack =
          distFromSide > 0.054 &&
          distFromSide < 0.092 &&
          y > 0.048 &&
          y < 3.508;
        inBand = onFrontTip || onBackTip || onSideTrack;
      }
    } else {
      const outerX = caseHalfWAtY(y);
      if (z > 0.495 && nz > 0.55) {
        const onBot = y > -0.888 && y < -0.862 && ax < outerX - 0.052;
        const onSides =
          ax > outerX - 0.092 &&
          ax < outerX - 0.052 &&
          y > -0.888 &&
          y < -0.456;
        inBand = onBot || onSides;
      } else if (z < -0.455 && nz < -0.55) {
        const onBot = y > -0.888 && y < -0.862 && ax < outerX - 0.05;
        const onTop = y > 0.785 && y < 0.875 && ax < outerX - 0.045;
        const onSides =
          ax > outerX - 0.096 && ax < outerX - 0.042 && y > -0.888 && y < 0.875;
        inBand = onBot || onTop || onSides;
      } else if (ax > outerX - 0.026 && Math.abs(nx) > 0.72) {
        const onVert = az > 0.39 && az < 0.432 && y > -0.888 && y < 0.875;
        const onBot = y > -0.888 && y < -0.858 && az < 0.432;
        const onTop = y > 0.775 && y < 0.875 && az < 0.432;
        inBand = onVert || onBot || onTop;
      }
    }

    if (!inBand) continue;

    const h =
      (x - (cur[i3] ?? 0)) * nx +
      (y - (cur[i3 + 1] ?? 0)) * ny +
      (z - (cur[i3 + 2] ?? 0)) * nz;

    stitchArr[i * 2] = smoothstep(0.00018, 0.00062, h);
    stitchArr[i * 2 + 1] = smoothstep(-0.00022, -0.0006, h);
  }

  geometry.setAttribute("aStitch", new THREE.BufferAttribute(stitchArr, 2));
}

/**
 * Maps flat leather UVs, splits outer vs lining material groups, and prepares
 * shared geometry once per page. On the Case mesh, hidden back-facing triangles
 * behind the upper front plate are filtered BEFORE computing vertex normals so
 * curved front corner seam vertices keep clean outward normals.
 */
function mapMesh(mesh: THREE.Mesh, cover: boolean): void {
  const geometry = mesh.geometry;
  if (geometry.userData.leatherMapped) return;
  const position = geometry.getAttribute("position");
  const rawIndex = geometry.getIndex();
  if (!rawIndex) throw new Error("Expected indexed leather geometry");

  const flapContourFn = cover ? sampleFlapOuterContour(mesh) : undefined;

  if (!cover) {
    const cleaned: number[] = [];
    for (let i = 0; i < rawIndex.count; i += 3) {
      const a = rawIndex.getX(i);
      const b = rawIndex.getX(i + 1);
      const c = rawIndex.getX(i + 2);
      const ax = position.getX(a),
        ay = position.getY(a),
        az = position.getZ(a);
      const bx = position.getX(b),
        by = position.getY(b),
        bz = position.getZ(b);
      const cx = position.getX(c),
        cy = position.getY(c),
        cz = position.getZ(c);
      const mx = (ax + bx + cx) / 3;
      const my = (ay + by + cy) / 3;
      const mz = (az + bz + cz) / 3;
      if (mz > 0.48 && my > -0.36 && my < 0.68 && Math.abs(mx) < 0.765) {
        const ux = bx - ax,
          uy = by - ay,
          uz = bz - az;
        const vx = cx - ax,
          vy = cy - ay,
          vz = cz - az;
        const fx = uy * vz - uz * vy;
        const fy = uz * vx - ux * vz;
        const fz = ux * vy - uy * vx;
        const flen = Math.hypot(fx, fy, fz) || 1;
        if (fz / flen < 0.25) continue;
      }
      cleaned.push(a, b, c);
    }
    geometry.setIndex(cleaned);
  }

  geometry.computeVertexNormals();
  const normal = geometry.getAttribute("normal");
  const uv = new Float32Array(position.count * 2);
  const grainUv = new Float32Array(position.count * 2);
  for (let i = 0; i < position.count; i++) {
    const x = position.getX(i),
      y = position.getY(i),
      z = position.getZ(i);
    uv[i * 2] = (x + WIDTH / 2) / WIDTH;
    uv[i * 2 + 1] = cover ? 1 - y / LENGTH : (y + 0.96) / HEIGHT;
    const side = !cover && Math.abs(normal.getX(i)) > 0.7;
    grainUv[i * 2] = (side ? z : x) * 2;
    grainUv[i * 2 + 1] = y * 2;
  }
  geometry.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
  geometry.setAttribute("uv1", new THREE.BufferAttribute(grainUv, 2));
  const index = geometry.getIndex();
  if (!index) throw new Error("Expected indexed leather geometry");
  const outer: number[] = [];
  const lining: number[] = [];
  for (let i = 0; i < index.count; i += 3) {
    const a = index.getX(i),
      b = index.getX(i + 1),
      c = index.getX(i + 2);
    const z = (position.getZ(a) + position.getZ(b) + position.getZ(c)) / 3;
    const nz = (normal.getZ(a) + normal.getZ(b) + normal.getZ(c)) / 3;
    const printed = cover ? z < 0.01 : z > 0.4 && nz > 0.15;
    (printed ? outer : lining).push(a, b, c);
  }
  geometry.setIndex([...outer, ...lining]);
  geometry.clearGroups();
  geometry.addGroup(0, outer.length, 0);
  geometry.addGroup(outer.length, lining.length, 1);
  computeStitchAttribute(mesh, cover, flapContourFn);
  geometry.userData.leatherMapped = true;
}

/** Fast separable 1D box blur over a vertical slice of a float heightfield. */
function blurFloat2D(
  src: Float32Array,
  w: number,
  h: number,
  radius: number,
  yStart = 0,
  yEnd = h,
): Float32Array {
  const tmp = new Float32Array(src.length);
  const dst = new Float32Array(src.length);
  const y0 = Math.max(0, yStart);
  const y1 = Math.min(h, yEnd);
  const diam = radius * 2 + 1;

  for (let y = 0; y < y0; y++) dst.set(src.subarray(y * w, (y + 1) * w), y * w);
  for (let y = y1; y < h; y++) dst.set(src.subarray(y * w, (y + 1) * w), y * w);

  for (let y = y0; y < y1; y++) {
    const row = y * w;
    let acc = 0;
    for (let dx = -radius; dx <= radius; dx++) {
      acc += src[row + Math.max(0, Math.min(w - 1, dx))] ?? 0;
    }
    for (let x = 0; x < w; x++) {
      tmp[row + x] = acc / diam;
      const addX = Math.min(w - 1, x + radius + 1);
      const subX = Math.max(0, x - radius);
      acc += (src[row + addX] ?? 0) - (src[row + subX] ?? 0);
    }
  }
  for (let x = 0; x < w; x++) {
    let acc = 0;
    for (let dy = -radius; dy <= radius; dy++) {
      const sy = Math.max(y0, Math.min(y1 - 1, y0 + dy));
      acc += tmp[sy * w + x] ?? 0;
    }
    for (let y = y0; y < y1; y++) {
      dst[y * w + x] = acc / diam;
      const addY = Math.min(y1 - 1, y + radius + 1);
      const subY = Math.max(y0, y - radius);
      acc += (tmp[addY * w + x] ?? 0) - (tmp[subY * w + x] ?? 0);
    }
  }
  return dst;
}

/** Converts height, AO, and roughness fields into tangent-space tooling maps. */
function bakeToolingMaps(
  heightField: Float32Array,
  aoField: Float32Array,
  roughField: Float32Array,
  w: number,
  h: number,
  normalCtx: CanvasRenderingContext2D,
  aoRoughCtx: CanvasRenderingContext2D,
  slopeGain = 5.5,
): void {
  const nImg = normalCtx.createImageData(w, h);
  const nData = nImg.data;
  const arImg = aoRoughCtx.createImageData(w, h);
  const arData = arImg.data;

  for (let y = 0; y < h; y++) {
    const ym = Math.max(0, y - 1) * w;
    const yc = y * w;
    const yp = Math.min(h - 1, y + 1) * w;
    for (let x = 0; x < w; x++) {
      const xm = Math.max(0, x - 1);
      const xp = Math.min(w - 1, x + 1);

      const tl = heightField[ym + xm] ?? 0.5;
      const tc = heightField[ym + x] ?? 0.5;
      const tr = heightField[ym + xp] ?? 0.5;
      const ml = heightField[yc + xm] ?? 0.5;
      const mr = heightField[yc + xp] ?? 0.5;
      const bl = heightField[yp + xm] ?? 0.5;
      const bc = heightField[yp + x] ?? 0.5;
      const br = heightField[yp + xp] ?? 0.5;

      // Canvas +X is +U (+T); Canvas +Y is -V (-B) because CanvasTexture flipY = true.
      const gx = (tl + 2 * ml + bl - (tr + 2 * mr + br)) * 0.125 * slopeGain;
      const gy = (bl + 2 * bc + br - (tl + 2 * tc + tr)) * 0.125 * slopeGain;
      const gz = 1.0;
      const invLen = 1.0 / Math.hypot(gx, gy, gz);

      const i4 = (yc + x) * 4;
      nData[i4] = Math.round((gx * invLen * 0.5 + 0.5) * 255);
      nData[i4 + 1] = Math.round((gy * invLen * 0.5 + 0.5) * 255);
      nData[i4 + 2] = Math.round((gz * invLen * 0.5 + 0.5) * 255);
      nData[i4 + 3] = 255;

      const ao = Math.max(0, Math.min(1, aoField[yc + x] ?? 1));
      const rMod = Math.max(0, Math.min(1, roughField[yc + x] ?? 1));
      const hClamped = Math.max(0, Math.min(1, heightField[yc + x] ?? 0.5));
      arData[i4] = Math.round(ao * 255);
      arData[i4 + 1] = Math.round(rMod * 255);
      arData[i4 + 2] = Math.round(hClamped * 255);
      arData[i4 + 3] = 255;
    }
  }
  normalCtx.putImageData(nImg, 0, 0);
  aoRoughCtx.putImageData(arImg, 0, 0);
}

/** Stamps whisper-subtle twin circular hidden-magnet impressions into tooling fields. */
function stampCircularMagnetsIntoFields(
  height: Float32Array,
  ao: Float32Array,
  rough: Float32Array,
  w: number,
  h: number,
  magnets: readonly MagnetImprint[],
  depthScale = 1.0,
): void {
  for (const { cx, cy, rx, ry } of magnets) {
    const padX = Math.ceil(rx * 1.45);
    const padY = Math.ceil(ry * 1.45);
    const x0 = Math.max(0, Math.floor(cx - padX));
    const x1 = Math.min(w - 1, Math.ceil(cx + padX));
    const y0 = Math.max(0, Math.floor(cy - padY));
    const y1 = Math.min(h - 1, Math.ceil(cy + padY));
    for (let y = y0; y <= y1; y++) {
      const dy = (y - cy) / ry;
      const row = y * w;
      for (let x = x0; x <= x1; x++) {
        const dx = (x - cx) / rx;
        const r = Math.hypot(dx, dy);
        if (r > 1.38) continue;
        const ringCrease = Math.exp(-Math.pow((r - 0.96) / 0.2, 2));
        const innerDisc = 1.0 - Math.max(0, Math.min(1, (r - 0.72) / 0.34));
        const outerShoulder = Math.exp(-Math.pow((r - 1.18) / 0.18, 2));
        const dh =
          (-ringCrease * 0.038 - innerDisc * 0.016 + outerShoulder * 0.012) *
          depthScale;
        const idx = row + x;
        height[idx] = Math.max(0.02, Math.min(0.95, (height[idx] ?? 0.5) + dh));
        ao[idx] = Math.max(
          0.55,
          Math.min(
            1.0,
            (ao[idx] ?? 1) -
              ringCrease * 0.048 * depthScale -
              innerDisc * 0.015 * depthScale,
          ),
        );
        rough[idx] = Math.max(
          0.45,
          Math.min(
            1.0,
            (rough[idx] ?? 1) -
              innerDisc * 0.09 * depthScale -
              ringCrease * 0.03 * depthScale,
          ),
        );
      }
    }
  }
}

/** Paints subtle compressed-hide shading for sewn-in hidden magnet impressions. */
function paintCircularMagnetShade(
  ctx: CanvasRenderingContext2D,
  magnets: readonly MagnetImprint[],
  alphaMul = 1.0,
): void {
  for (const { cx, cy, rx, ry } of magnets) {
    ctx.save();
    ctx.translate(cx, cy);
    ctx.scale(rx, ry);
    const ring = ctx.createRadialGradient(0, 0, 0.76, 0, 0, 1.24);
    ring.addColorStop(0.0, "rgba(0, 0, 0, 0)");
    ring.addColorStop(0.32, `rgba(6, 4, 2, ${0.14 * alphaMul})`);
    ring.addColorStop(0.48, `rgba(4, 3, 2, ${0.22 * alphaMul})`);
    ring.addColorStop(0.68, `rgba(6, 4, 2, ${0.08 * alphaMul})`);
    ring.addColorStop(0.84, `rgba(255, 240, 215, ${0.05 * alphaMul})`);
    ring.addColorStop(1.0, "rgba(0, 0, 0, 0)");
    ctx.fillStyle = ring;
    ctx.beginPath();
    ctx.arc(0, 0, 1.25, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }
}

/** Builds UV0 deboss normal and AO/roughness maps for mana symbols and magnets. */
function buildDebossToolingMaps(
  debossMaskCanvas: HTMLCanvasElement | null,
  w: number,
  h: number,
  normalCtx: CanvasRenderingContext2D,
  aoRoughCtx: CanvasRenderingContext2D,
  yStart: number,
  yEnd: number,
  slopeGain: number,
  magnets: readonly MagnetImprint[],
): void {
  const n = w * h;
  const height = new Float32Array(n);
  const ao = new Float32Array(n);
  const rough = new Float32Array(n);
  height.fill(0.5);
  ao.fill(1.0);
  rough.fill(1.0);

  if (debossMaskCanvas) {
    const maskCtx = debossMaskCanvas.getContext("2d");
    if (maskCtx) {
      const mPx = maskCtx.getImageData(0, 0, w, h).data;
      const symRaw = new Float32Array(n);
      for (let y = yStart; y < yEnd; y++) {
        const row = y * w;
        for (let x = 0; x < w; x++) {
          symRaw[row + x] = (mPx[(row + x) * 4 + 3] ?? 0) / 255;
        }
      }
      const tight = blurFloat2D(
        blurFloat2D(symRaw, w, h, 2, yStart, yEnd),
        w,
        h,
        2,
        yStart,
        yEnd,
      );
      const wide = blurFloat2D(
        blurFloat2D(symRaw, w, h, 5, yStart, yEnd),
        w,
        h,
        4,
        yStart,
        yEnd,
      );

      for (let y = yStart; y < yEnd; y++) {
        const row = y * w;
        for (let x = 0; x < w; x++) {
          const idx = row + x;
          const tVal = tight[idx] ?? 0;
          const wVal = wide[idx] ?? 0;
          const sInner = Math.min(1, tVal * 1.25);
          const outerRim = Math.max(0, wVal - tVal) * 1.9;
          const wallBand = 4.0 * tVal * (1.0 - tVal);

          height[idx] = Math.max(
            0.02,
            Math.min(0.95, 0.5 - sInner * 0.42 + outerRim * 0.09),
          );
          ao[idx] = 1.0 - wallBand * 0.36 - sInner * 0.16;
          rough[idx] = 1.0 - sInner * 0.36 - outerRim * 0.14;
        }
      }
    }
  }

  if (magnets.length > 0) {
    stampCircularMagnetsIntoFields(height, ao, rough, w, h, magnets, 1.0);
  }

  bakeToolingMaps(height, ao, rough, w, h, normalCtx, aoRoughCtx, slopeGain);
}

type SharedInputs = {
  normal: HTMLImageElement;
  roughness: HTMLImageElement;
  glyphs: Map<SymbolCode, GlyphPair>;
  /** The leather photograph as a neutral grain, for tinting and wear. */
  grain: CanvasRenderingContext2D;
  /** Refined cleaner ink abrasion mask (0.97 - abrasion * 0.10). */
  inkMask: CanvasRenderingContext2D;
  /** The Spellwake mark (the approved SVG), stamped on a box with no face card. */
  emblem: HTMLImageElement;
};
let shared: Promise<SharedInputs> | null = null;

/**
 * Everything boxes have in common: the leather maps, the official mana
 * glyphs and masks, and the painted grain and ink mask. Prepared once per page;
 * a failed read is retried on the next call.
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
          SYMBOL_CODES.map(async (code): Promise<[SymbolCode, GlyphPair]> => [
            code,
            await glyphPair(artworkUrl, code),
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
      inkPixels.data[i] = 255;
      inkPixels.data[i + 1] = 255;
      inkPixels.data[i + 2] = 255;
      inkPixels.data[i + 3] = Math.round(255 * (0.97 - abrasion * 0.1));
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
  linerToolNormTex: THREE.Texture;
  linerToolARTex: THREE.Texture;
};
let leatherMaps: SharedMaps | null = null;

/**
 * The leather maps and geometry-invariant inner-flap magnet tooling maps made
 * once per page so the shared WebGL renderer uploads them once.
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

  const linerToolNorm = surface(2048, SIZE);
  const linerToolAR = surface(2048, SIZE);
  buildDebossToolingMaps(
    null,
    SIZE,
    2048,
    linerToolNorm,
    linerToolAR,
    1780,
    2040,
    5.4,
    LINER_MAGNETS,
  );
  const linerToolNormTex = new THREE.CanvasTexture(linerToolNorm.canvas);
  linerToolNormTex.colorSpace = THREE.NoColorSpace;
  linerToolNormTex.anisotropy = 4;
  linerToolNormTex.needsUpdate = true;
  const linerToolARTex = new THREE.CanvasTexture(linerToolAR.canvas);
  linerToolARTex.colorSpace = THREE.NoColorSpace;
  linerToolARTex.anisotropy = 4;
  linerToolARTex.needsUpdate = true;

  leatherMaps = {
    normalMap,
    roughnessMap,
    wearMap,
    grainMap,
    linerToolNormTex,
    linerToolARTex,
  };
  return leatherMaps;
}

export type BoxTextures = {
  repaint: (appearance: BoxAppearance) => void;
  /** Updates dynamic closed-flap contact shadow attenuation (`0` = closed, `1` = open). */
  setOpenProgress: (openProgress: number) => void;
  dispose: () => void;
};

type LeatherMaterialOptions = {
  lining?: boolean;
  cover?: boolean;
  detail?: boolean;
  toolingNormalMap?: THREE.Texture;
  toolingAoRoughMap?: THREE.Texture;
  toolingStrength?: number;
  dynamicFlapShadow?: boolean;
};

/**
 * Applies the refined Spellwake leather finish, clearer cover art integration,
 * physically debossed world-square mana symbols, twin subtle hidden-magnet
 * impressions, and 360-degree imprinted charcoal-black stitching to the 3D box.
 * `onRepaint` is called when face art finishes loading so the caller can redraw.
 */
export async function createBoxTextures(
  root: THREE.Object3D,
  artworkUrl: string,
  signal?: AbortSignal,
  onRepaint?: () => void,
): Promise<BoxTextures> {
  signal?.throwIfAborted();
  const inputs = await sharedInputs(artworkUrl);
  signal?.throwIfAborted();
  const { glyphs, grain, inkMask } = inputs;
  const {
    normalMap,
    roughnessMap,
    wearMap,
    grainMap,
    linerToolNormTex,
    linerToolARTex,
  } = sharedMaps(inputs);

  const front = surface(SIZE, SIZE);
  const wrap = surface(2048, SIZE);
  const plain = surface(SIZE, SIZE);
  const liner = surface(2048, SIZE);

  const frontToolNorm = surface(SIZE, SIZE);
  const frontToolAR = surface(SIZE, SIZE);
  const frontManaMask = surface(SIZE, SIZE);
  const tintScratch = surface(256, 256);

  const maps = [front, wrap, plain, liner].map((ctx) => {
    const texture = new THREE.CanvasTexture(ctx.canvas);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.anisotropy = 4;
    return texture;
  });
  const frontToolNormTex = new THREE.CanvasTexture(frontToolNorm.canvas);
  frontToolNormTex.colorSpace = THREE.NoColorSpace;
  frontToolNormTex.anisotropy = 4;
  const frontToolARTex = new THREE.CanvasTexture(frontToolAR.canvas);
  frontToolARTex.colorSpace = THREE.NoColorSpace;
  frontToolARTex.anisotropy = 4;

  const uFlapOpen = { value: 0.0 };

  const material = (
    map: THREE.Texture | undefined,
    opts: LeatherMaterialOptions = {},
  ): THREE.MeshStandardMaterial => {
    const {
      lining = false,
      cover = false,
      detail = true,
      toolingNormalMap,
      toolingAoRoughMap,
      toolingStrength = 1.0,
      dynamicFlapShadow = false,
    } = opts;

    const result = new THREE.MeshStandardMaterial({
      ...(map ? { map } : {}),
      color: lining ? 0x9c958b : 0xffffff,
      normalMap,
      normalScale: new THREE.Vector2(
        lining ? 0.44 : 0.32,
        lining ? 0.44 : 0.32,
      ),
      roughnessMap,
      roughness: lining ? 0.95 : 0.9,
      metalness: 0.04,
    });
    result.onBeforeCompile = (shader) => {
      shader.uniforms.wearMap = { value: wearMap };
      shader.uniforms.grainMap = { value: grainMap };
      shader.uniforms.surfaceDetail = { value: detail ? 1.0 : 0.0 };
      shader.uniforms.printedCover = { value: cover && !lining ? 1.0 : 0.0 };
      shader.uniforms.coverSurface = { value: cover ? 1.0 : 0.0 };
      shader.uniforms.wearStrength = { value: lining ? 0.85 : 1.0 };
      shader.uniforms.hasToolingMap = { value: toolingNormalMap ? 1.0 : 0.0 };
      shader.uniforms.toolingNormalMap = {
        value: toolingNormalMap ?? normalMap,
      };
      shader.uniforms.toolingAoRoughMap = {
        value: toolingAoRoughMap ?? roughnessMap,
      };
      shader.uniforms.toolingStrength = { value: toolingStrength };
      shader.uniforms.hasDynamicFlapShadow = {
        value: dynamicFlapShadow ? 1.0 : 0.0,
      };
      shader.uniforms.uFlapOpen = uFlapOpen;

      shader.vertexShader =
        "attribute vec2 aStitch;\nvarying vec2 vWearUv;\nvarying vec2 vToolingUv;\nvarying vec3 vWearPosition;\nvarying vec2 vStitch;\n" +
        shader.vertexShader;
      shader.vertexShader = shader.vertexShader.replace(
        "#include <uv_vertex>",
        "#include <uv_vertex>\nvWearUv = uv1 * 0.4;\nvToolingUv = uv;\nvWearPosition = position;\nvStitch = aStitch;",
      );
      shader.fragmentShader =
        "uniform sampler2D wearMap;\nuniform sampler2D grainMap;\nuniform float surfaceDetail;\nuniform float printedCover;\nuniform float coverSurface;\nuniform float wearStrength;\nuniform float hasToolingMap;\nuniform sampler2D toolingNormalMap;\nuniform sampler2D toolingAoRoughMap;\nuniform float toolingStrength;\nuniform float hasDynamicFlapShadow;\nuniform float uFlapOpen;\nvarying vec2 vWearUv;\nvarying vec2 vToolingUv;\nvarying vec3 vWearPosition;\nvarying vec2 vStitch;\n" +
        shader.fragmentShader;

      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <map_fragment>",
        `#include <map_fragment>
        float xEdge = abs(abs(vWearPosition.x) - mix(0.78, 0.74, coverSurface));
        float caseY = min(abs(vWearPosition.y + 0.94), abs(vWearPosition.y - 0.90));
        float caseZ = abs(abs(vWearPosition.z) - 0.51);
        float foldEnd = min(min(abs(vWearPosition.y - 0.04), abs(vWearPosition.y - 1.18)),
                            min(abs(vWearPosition.y - 2.378), abs(vWearPosition.y - 3.52)));
        float cornerDistance = mix(length(vec3(xEdge, caseY, caseZ)), length(vec2(xEdge, foldEnd)), coverSurface);
        float corner = 1.0 - smoothstep(0.025, 0.22, cornerDistance);
        float caseEdge = min(length(vec2(xEdge, caseY)), min(length(vec2(xEdge, caseZ)), length(vec2(caseY, caseZ))));
        float edge = 1.0 - smoothstep(0.012, 0.07, mix(caseEdge, xEdge, coverSurface));

        // Identify debossed mana ink via toolAR.b so patina spans 100% of the front leather without dulling glyphs.
        float toolRoughMod = 1.0;
        float toolAo = 1.0;
        float manaInkMask = 0.0;
        if (hasToolingMap > 0.5) {
          vec3 toolAR = texture2D(toolingAoRoughMap, vToolingUv).rgb;
          toolAo = toolAR.r;
          toolRoughMod = toolAR.g;
          manaInkMask = 1.0 - smoothstep(0.18, 0.34, toolAR.b);
        }

        float detailMask = surfaceDetail * (1.0 - printedCover * smoothstep(2.04, 2.26, vWearPosition.y));
        detailMask *= (1.0 - manaInkMask * 0.85);

        // Continuous dark oil-rubbed weathered patina across the full hide (no chalky white spray).
        float wearMask = clamp((mix(0.26, 0.64, detailMask) + corner * 0.72 + edge * 0.22) * wearStrength, 0.0, 1.0);
        wearMask *= (1.0 - manaInkMask * 0.88);
        float wearNorm = clamp((texture2D(wearMap, vWearUv).r - 0.22) / 0.42, 0.0, 1.0);
        float wearSample = mix(0.12, 0.53, wearNorm);
        float wear = mix(0.5, wearSample, wearMask);

        float leatherGrain = texture2D(grainMap, vWearUv * 1.6).r;
        float baseLift = mix(0.76, 0.87, max(printedCover * smoothstep(2.14, 2.36, vWearPosition.y), manaInkMask));
        diffuseColor.rgb *= baseLift * mix(1.0, 0.46 + leatherGrain * 0.64, detailMask);
        float panelShade = mix(1.0, 0.80 + 0.20 * (1.0 - edge), detailMask);
        diffuseColor.rgb *= panelShade;

        vec3 rubbedTone = min(diffuseColor.rgb * 1.12 + vec3(0.010, 0.007, 0.003), vec3(0.72));
        diffuseColor.rgb = wear <= 0.5
          ? diffuseColor.rgb * mix(0.40, 1.0, wear * 2.0)
          : mix(diffuseColor.rgb, rubbedTone, (wear - 0.5) * 0.26);

        // UV0 tooling crevice occlusion for debossed mana walls and subtle magnet impressions.
        diffuseColor.rgb *= mix(1.0, toolAo, 0.92);

        // Imprinted dark charcoal-black waxed thread along the real sculpted 3D stitches.
        float stitchGroove = smoothstep(0.30, 0.82, vStitch.y);
        float stitchThread = smoothstep(0.24, 0.76, vStitch.x);
        diffuseColor.rgb *= mix(1.0, 0.56, stitchGroove * 0.85);
        vec3 waxedCharcoalThread = vec3(0.016, 0.019, 0.026);
        diffuseColor.rgb = mix(diffuseColor.rgb, waxedCharcoalThread, stitchThread * 0.96);

        // Dynamic soft closed-flap contact shadow on front apron; fades out as the lid opens.
        if (hasDynamicFlapShadow > 0.5) {
          float closedAmt = 1.0 - smoothstep(0.0, 0.40, uFlapOpen);
          float belowFlap = (-0.375) - vWearPosition.y;
          float contactShadow = smoothstep(-0.025, 0.018, belowFlap) * (1.0 - smoothstep(0.018, 0.145, belowFlap));
          diffuseColor.rgb *= (1.0 - contactShadow * 0.32 * closedAmt);
        }
      `,
      );

      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <normal_fragment_maps>",
        `#include <normal_fragment_maps>
        if (hasToolingMap > 0.5) {
          vec3 toolN = texture2D(toolingNormalMap, vToolingUv).xyz * 2.0 - 1.0;
          toolN.xy *= toolingStrength;
          mat3 tbnTool = getTangentFrame( - vViewPosition, normal, vToolingUv );
          #ifdef DOUBLE_SIDED
            tbnTool[0] *= faceDirection;
            tbnTool[1] *= faceDirection;
          #endif
          normal = normalize( tbnTool * normalize( toolN ) );
        }
      `,
      );

      shader.fragmentShader = shader.fragmentShader.replace(
        "#include <roughnessmap_fragment>",
        `#include <roughnessmap_fragment>
        roughnessFactor *= mix(1.0, 0.78, smoothstep(0.45, 0.65, wear));
        roughnessFactor = clamp(roughnessFactor * toolRoughMod, 0.28, 0.98);
        roughnessFactor = mix(roughnessFactor, 0.38, stitchThread * 0.88);
      `,
      );
    };
    result.customProgramCacheKey = () =>
      `refined-leather-v5-${toolingNormalMap ? "tool" : "plain"}-${dynamicFlapShadow ? "dyn" : "stat"}`;
    return result;
  };

  const frontMaterial = material(maps[0], {
    detail: true,
    toolingNormalMap: frontToolNormTex,
    toolingAoRoughMap: frontToolARTex,
    toolingStrength: 1.25,
    dynamicFlapShadow: true,
  });
  const wrapMaterial = material(maps[1], {
    cover: true,
    detail: true,
  });
  const plainMaterial = material(maps[2], {
    detail: true,
  });
  const liningMaterial = material(maps[3], {
    lining: true,
    cover: true,
    detail: true,
    toolingNormalMap: linerToolNormTex,
    toolingAoRoughMap: linerToolARTex,
    toolingStrength: 1.05,
  });

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

  function fillDyedLeather(
    ctx: CanvasRenderingContext2D,
    ramp: readonly (readonly [number, number, number])[],
    softBurnish: boolean,
    verticalDepth: boolean,
    applyUv0Grain: boolean,
  ): void {
    const w = ctx.canvas.width;
    const h = ctx.canvas.height;
    ctx.clearRect(0, 0, w, h);
    for (let x = 0; x < w; x++) {
      const color = ramp[x] ?? [0.5, 0.5, 0.5];
      ctx.fillStyle = `rgb(${color.map((channel) => Math.round(channel * 255)).join(" ")})`;
      ctx.fillRect(x, 0, 1, h);
    }
    if (applyUv0Grain) {
      ctx.save();
      ctx.globalCompositeOperation = "multiply";
      const pattern = ctx.createPattern(grain.canvas, "repeat");
      if (pattern) {
        pattern.setTransform(new DOMMatrix().scale(0.5));
        ctx.fillStyle = pattern;
        ctx.fillRect(0, 0, w, h);
      }
      ctx.restore();
    }
    if (verticalDepth) {
      const depth = ctx.createLinearGradient(0, 0, 0, h);
      depth.addColorStop(0, "rgba(0,0,0,.34)");
      depth.addColorStop(0.18, "rgba(0,0,0,.10)");
      depth.addColorStop(0.76, "rgba(0,0,0,.16)");
      depth.addColorStop(1, "rgba(0,0,0,.38)");
      ctx.fillStyle = depth;
      ctx.fillRect(0, 0, w, h);
    }
    if (softBurnish) {
      const hEdge = ctx.createLinearGradient(0, 0, w, 0);
      hEdge.addColorStop(0, "rgba(8,5,3,.56)");
      hEdge.addColorStop(0.055, "rgba(8,5,3,.24)");
      hEdge.addColorStop(0.1, "rgba(8,5,3,0)");
      hEdge.addColorStop(0.9, "rgba(8,5,3,0)");
      hEdge.addColorStop(0.945, "rgba(8,5,3,.24)");
      hEdge.addColorStop(1, "rgba(8,5,3,.56)");
      ctx.fillStyle = hEdge;
      ctx.fillRect(0, 0, w, h);
    }
  }

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

    fillDyedLeather(front, ramp, true, true, true);
    fillDyedLeather(wrap, ramp, false, false, true);
    fillDyedLeather(plain, ramp, false, true, false);
    fillDyedLeather(liner, ramp, false, true, true);

    paintCircularMagnetShade(front, FRONT_MAGNETS, 0.24);
    paintCircularMagnetShade(liner, LINER_MAGNETS, 0.28);

    const source = faceArt ? loaded.get(faceArt) : null;
    if (source) {
      const x = 0;
      const width = SIZE;
      const y = (2.174 / LENGTH) * 2048;
      const height = ((LENGTH - 2.174) / LENGTH) * 2048;
      const print = surface(2048, SIZE);
      print.save();
      print.beginPath();
      print.rect(x, y, width, height);
      print.clip();
      print.filter = "saturate(1.06) contrast(1.05) brightness(1.04)";
      const ratio = WIDTH / (LENGTH - 2.174);
      const cropWidth = Math.min(
        source.naturalWidth,
        source.naturalHeight * ratio,
      );
      const cropHeight = cropWidth / ratio;
      print.drawImage(
        source,
        (source.naturalWidth - cropWidth) / 2,
        (source.naturalHeight - cropHeight) * 0.44,
        cropWidth,
        cropHeight,
        x,
        y,
        width,
        height,
      );
      print.filter = "none";
      print.globalCompositeOperation = "multiply";
      print.globalAlpha = 0.24;
      print.drawImage(grain.canvas, x, y, width, height);
      print.globalAlpha = 1;
      print.globalCompositeOperation = "destination-in";
      print.drawImage(inkMask.canvas, x, y, width, height);
      const vFade = print.createLinearGradient(0, y, 0, y + height);
      vFade.addColorStop(0, "rgba(0,0,0,0)");
      vFade.addColorStop(0.05, "rgba(0,0,0,.30)");
      vFade.addColorStop(0.14, "rgba(0,0,0,.91)");
      vFade.addColorStop(0.89, "rgba(0,0,0,.91)");
      vFade.addColorStop(0.955, "rgba(0,0,0,.42)");
      vFade.addColorStop(1, "rgba(0,0,0,.06)");
      print.fillStyle = vFade;
      print.fillRect(x, y, width, height);
      const hFade = print.createLinearGradient(0, 0, SIZE, 0);
      hFade.addColorStop(0, "rgba(0,0,0,0)");
      hFade.addColorStop(0.05, "rgba(0,0,0,.08)");
      hFade.addColorStop(0.11, "rgba(0,0,0,1)");
      hFade.addColorStop(0.89, "rgba(0,0,0,1)");
      hFade.addColorStop(0.95, "rgba(0,0,0,.08)");
      hFade.addColorStop(1, "rgba(0,0,0,0)");
      print.fillStyle = hFade;
      print.fillRect(x, y, width, height);
      print.restore();

      wrap.save();
      wrap.globalCompositeOperation = "multiply";
      wrap.globalAlpha = 0.14;
      wrap.drawImage(print.canvas, 0, 0);
      wrap.restore();
      wrap.drawImage(print.canvas, 0, 0);
    } else {
      // A deck with no cards has no face card yet; stamp the approved Spellwake mark.
      const flapTop = (2.174 / LENGTH) * 2048;
      const flapHeight = ((LENGTH - 2.174) / LENGTH) * 2048;
      const height = flapHeight * 0.4;
      const width =
        (height * inputs.emblem.naturalWidth) / inputs.emblem.naturalHeight;
      const left = (SIZE - width) / 2;
      const top = flapTop + flapHeight * 0.58 - height / 2;
      const mark = surface(2048, SIZE);
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

    // Warm oil-rubbed edge burnish along both flap side edges.
    const wrapEdge = wrap.createLinearGradient(0, 0, SIZE, 0);
    wrapEdge.addColorStop(0, "rgba(8,5,3,.56)");
    wrapEdge.addColorStop(0.055, "rgba(8,5,3,.24)");
    wrapEdge.addColorStop(0.1, "rgba(8,5,3,0)");
    wrapEdge.addColorStop(0.9, "rgba(8,5,3,0)");
    wrapEdge.addColorStop(0.945, "rgba(8,5,3,.24)");
    wrapEdge.addColorStop(1, "rgba(8,5,3,.56)");
    wrap.fillStyle = wrapEdge;
    wrap.fillRect(0, 0, SIZE, 2048);

    // Organic bottom-edge burnish on the front apron.
    const bottomRim = front.createLinearGradient(0, 880, 0, 1024);
    bottomRim.addColorStop(0, "rgba(8,5,3,0)");
    bottomRim.addColorStop(1, "rgba(8,5,3,.44)");
    front.fillStyle = bottomRim;
    front.fillRect(0, 880, SIZE, 144);

    // Physically debossed hot-stamped mana symbols in uniform 1:1 world-square cells.
    frontManaMask.clearRect(0, 0, SIZE, SIZE);
    const symCodes: readonly SymbolCode[] = ordered.length ? ordered : ["C"];
    const symbols = symCodes
      .map((code) => {
        const pair = glyphs.get(code);
        return pair ? { code, color: pair.color, mask: pair.mask } : undefined;
      })
      .filter(
        (entry): entry is { code: SymbolCode; color: Glyph; mask: Glyph } =>
          entry !== undefined,
      );

    const uvAspect = HEIGHT / WIDTH;
    const count = symbols.length;
    const cellH = count >= 5 ? 86 : count === 4 ? 94 : count === 3 ? 100 : 104;
    const cellW = cellH * uvAspect;
    const cellGap = count >= 5 ? 44 : count === 4 ? 54 : count === 3 ? 62 : 74;
    const bandTop = ((0.96 + 0.4) / HEIGHT) * SIZE;
    const bandBottom = ((0.96 + 0.87) / HEIGHT) * SIZE;
    const cellTop = (bandTop + bandBottom - cellH) / 2;
    const totalRowW = count * cellW + Math.max(0, count - 1) * cellGap;
    let cellLeft = (SIZE - totalRowW) / 2;

    for (const sym of symbols) {
      const g = sym.color;
      const m = sym.mask;
      const aspect = g.width / g.height;
      const optScale = MANA_OPTICAL_SCALE[sym.code];
      let drawW: number;
      let drawH: number;
      if (aspect >= 1) {
        drawW = cellW * optScale;
        drawH = (cellH / aspect) * optScale;
      } else {
        drawH = cellH * optScale;
        drawW = cellW * aspect * optScale;
      }
      const gx = cellLeft + (cellW - drawW) * 0.5;
      const gy = cellTop + (cellH - drawH) * 0.5;

      frontManaMask.drawImage(
        m.image,
        m.x,
        m.y,
        m.width,
        m.height,
        gx,
        gy,
        drawW,
        drawH,
      );

      const drawTintedMask = (
        fillStyle: string,
        dx: number,
        dy: number,
        blurPx: number,
        alpha: number,
      ): void => {
        tintScratch.clearRect(0, 0, 256, 256);
        tintScratch.globalCompositeOperation = "source-over";
        tintScratch.drawImage(
          m.image,
          m.x,
          m.y,
          m.width,
          m.height,
          16,
          16,
          224,
          224,
        );
        tintScratch.globalCompositeOperation = "source-in";
        tintScratch.fillStyle = fillStyle;
        tintScratch.fillRect(0, 0, 256, 256);
        tintScratch.globalCompositeOperation = "source-over";

        front.save();
        front.globalAlpha = alpha;
        if (blurPx > 0) front.filter = `blur(${blurPx}px)`;
        const padX = (drawW / 224) * 16;
        const padY = (drawH / 224) * 16;
        front.drawImage(
          tintScratch.canvas,
          gx - padX + dx,
          gy - padY + dy,
          drawW + padX * 2,
          drawH + padY * 2,
        );
        front.restore();
      };

      drawTintedMask("#f6ead4", 2.0, 2.6, 2.2, 0.24);
      drawTintedMask("#09070b", 0, 0, 2.8, 0.56);
      drawTintedMask("#040307", -1.6, -2.0, 1.4, 0.62);

      const bw = Math.ceil(drawW + 16);
      const bh = Math.ceil(drawH + 16);
      const glyphBed = surface(bh, bw);
      glyphBed.drawImage(
        g.image,
        g.x,
        g.y,
        g.width,
        g.height,
        8,
        8,
        drawW,
        drawH,
      );
      glyphBed.globalCompositeOperation = "source-atop";
      const innerGrad = glyphBed.createLinearGradient(0, 6, 0, drawH + 8);
      innerGrad.addColorStop(0.0, "rgba(0, 0, 0, 0.36)");
      innerGrad.addColorStop(0.24, "rgba(0, 0, 0, 0.08)");
      innerGrad.addColorStop(0.76, "rgba(0, 0, 0, 0.02)");
      innerGrad.addColorStop(1.0, "rgba(255, 245, 224, 0.16)");
      glyphBed.fillStyle = innerGrad;
      glyphBed.fillRect(0, 0, bw, bh);

      glyphBed.globalCompositeOperation = "multiply";
      glyphBed.globalAlpha = 0.2;
      glyphBed.drawImage(
        grain.canvas,
        gx,
        gy,
        drawW,
        drawH,
        8,
        8,
        drawW,
        drawH,
      );
      glyphBed.globalAlpha = 1.0;
      glyphBed.globalCompositeOperation = "destination-in";
      glyphBed.drawImage(
        m.image,
        m.x,
        m.y,
        m.width,
        m.height,
        8,
        8,
        drawW,
        drawH,
      );

      front.save();
      front.globalAlpha = 0.96;
      front.drawImage(glyphBed.canvas, gx - 8, gy - 8);
      front.restore();

      cellLeft += cellW + cellGap;
    }

    buildDebossToolingMaps(
      frontManaMask.canvas,
      SIZE,
      SIZE,
      frontToolNorm,
      frontToolAR,
      500,
      990,
      6.8,
      FRONT_MAGNETS,
    );

    for (const map of maps) map.needsUpdate = true;
    frontToolNormTex.needsUpdate = true;
    frontToolARTex.needsUpdate = true;
  }

  repaint(DEFAULT_APPEARANCE);

  return {
    repaint,
    setOpenProgress(openProgress: number): void {
      uFlapOpen.value = THREE.MathUtils.clamp(openProgress, 0, 1);
    },
    dispose: () => {
      disposed = true;
      for (const map of maps) map.dispose();
      frontToolNormTex.dispose();
      frontToolARTex.dispose();
    },
  };
}
