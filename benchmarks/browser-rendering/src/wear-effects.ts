/**
 * Canonical 3D and 2D wear-and-tear cosmetic engine for Spellwake cards and sleeves.
 *
 * Near Mint (tier "nm") is always bypassed with zero surface or geometry distortion.
 * Higher tiers apply deterministic, seed-driven masks and 3D deformations so a whole
 * deck shares one consistent wear signature while players can reroll their pattern
 * seed in Cosmetics.
 */
import * as THREE from "three";
import {
  CARD_CORNER_RADIUS,
  CARD_HEIGHT,
  CARD_WIDTH,
  SLEEVE_HEIGHT,
  SLEEVE_WIDTH,
} from "./card-geometry";

export type WearTierId = "nm" | "lp" | "mp" | "hp" | "dmg" | "unplayable";

export type WearEffectSelection = {
  tier: WearTierId;
  seed?: number | undefined;
};

export type CardWearTierSpec = {
  readonly id: WearTierId;
  readonly label: string;
  readonly wear: number;
  readonly bend: number;
  readonly defaultSeed: number;
};

export type SleeveWearTierSpec = {
  readonly id: WearTierId;
  readonly label: string;
  readonly cornerBend: number;
  readonly grimeHaze: number;
  readonly lipWave: number;
  readonly defaultSeed: number;
};

export const CARD_WEAR_TIERS: readonly CardWearTierSpec[] = [
  { id: "nm", label: "Near Mint", wear: 0, bend: 0, defaultSeed: 0 },
  { id: "lp", label: "Lightly Played", wear: 0.31, bend: 0, defaultSeed: 329 },
  {
    id: "mp",
    label: "Moderately Played",
    wear: 0.5,
    bend: 0,
    defaultSeed: 751,
  },
  {
    id: "hp",
    label: "Heavily Played",
    wear: 0.72,
    bend: 0.28,
    defaultSeed: 456,
  },
  { id: "dmg", label: "Damaged", wear: 0.86, bend: 0.58, defaultSeed: 895 },
  {
    id: "unplayable",
    label: "Unplayable",
    wear: 1.0,
    bend: 1.0,
    defaultSeed: 985,
  },
];

export const SLEEVE_WEAR_TIERS: readonly SleeveWearTierSpec[] = [
  {
    id: "nm",
    label: "Near Mint",
    cornerBend: 0,
    grimeHaze: 0,
    lipWave: 0,
    defaultSeed: 0,
  },
  {
    id: "lp",
    label: "Lightly Played",
    cornerBend: 0.3,
    grimeHaze: 0.12,
    lipWave: 0.2,
    defaultSeed: 141,
  },
  {
    id: "mp",
    label: "Moderately Played",
    cornerBend: 0.54,
    grimeHaze: 0.34,
    lipWave: 0.4,
    defaultSeed: 317,
  },
  {
    id: "hp",
    label: "Heavily Played",
    cornerBend: 0.74,
    grimeHaze: 0.6,
    lipWave: 0.64,
    defaultSeed: 523,
  },
  {
    id: "dmg",
    label: "Damaged",
    cornerBend: 0.86,
    grimeHaze: 0.8,
    lipWave: 0.8,
    defaultSeed: 739,
  },
  {
    id: "unplayable",
    label: "Unplayable",
    cornerBend: 1.0,
    grimeHaze: 1.0,
    lipWave: 1.0,
    defaultSeed: 947,
  },
];

export const WEAR_SEED_MIN = 1;
export const WEAR_SEED_MAX = 999;

export function normalizeWearSeed(
  seed: number | undefined,
  fallback: number,
): number {
  if (seed === undefined || !Number.isFinite(seed)) return fallback;
  const rounded = Math.round(seed);
  if (rounded < WEAR_SEED_MIN || rounded > WEAR_SEED_MAX) return fallback;
  return rounded;
}

export function cardWearTierSpec(tier: WearTierId): CardWearTierSpec {
  return (
    CARD_WEAR_TIERS.find((entry) => entry.id === tier) ?? CARD_WEAR_TIERS[0]!
  );
}

export function sleeveWearTierSpec(tier: WearTierId): SleeveWearTierSpec {
  return (
    SLEEVE_WEAR_TIERS.find((entry) => entry.id === tier) ??
    SLEEVE_WEAR_TIERS[0]!
  );
}

const CARD_OFFSET_Y = -0.028;
const WORN_SLEEVE_W = SLEEVE_WIDTH + 0.06;
const WORN_SLEEVE_H = SLEEVE_HEIGHT + 0.09;
const WORN_SLEEVE_R = 0.085;
const WORN_SLEEVE_HALF_THICK = 0.021;

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function makeNoise2D(seed: number): {
  sample: (x: number, y: number) => number;
  fbm: (x: number, y: number, octaves?: number) => number;
} {
  const rand = mulberry32(seed);
  const perm = new Uint8Array(512);
  const vals = new Float32Array(256);
  for (let i = 0; i < 256; i++) {
    perm[i] = i;
    vals[i] = rand();
  }
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    const tmp = perm[i]!;
    perm[i] = perm[j]!;
    perm[j] = tmp;
  }
  for (let i = 0; i < 256; i++) perm[i + 256] = perm[i]!;
  const fade = (t: number) => t * t * t * (t * (t * 6 - 15) + 10);
  const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
  const sample = (x: number, y: number): number => {
    const xi = Math.floor(x) & 255;
    const yi = Math.floor(y) & 255;
    const xf = x - Math.floor(x);
    const yf = y - Math.floor(y);
    const u = fade(xf);
    const v = fade(yf);
    const aa = perm[(perm[xi]! + yi) & 511]!;
    const ba = perm[(perm[(xi + 1) & 255]! + yi) & 511]!;
    const ab = perm[(perm[xi]! + yi + 1) & 511]!;
    const bb = perm[(perm[(xi + 1) & 255]! + yi + 1) & 511]!;
    return lerp(
      lerp(vals[aa]!, vals[ba]!, u),
      lerp(vals[ab]!, vals[bb]!, u),
      v,
    );
  };
  const fbm = (x: number, y: number, octaves = 4): number => {
    let sum = 0;
    let amp = 0.5;
    let freq = 1;
    let norm = 0;
    for (let o = 0; o < octaves; o++) {
      sum += sample(x * freq, y * freq) * amp;
      norm += amp;
      amp *= 0.5;
      freq *= 2.03;
    }
    return sum / norm;
  };
  return { sample, fbm };
}

function smoothstep(e0: number, e1: number, x: number): number {
  const t = Math.max(0, Math.min(1, (x - e0) / Math.max(1e-6, e1 - e0)));
  return t * t * (3 - 2 * t);
}

function clusterEdge(t: number): number {
  const cosMap = 0.5 - 0.5 * Math.cos(t * Math.PI);
  return t * 0.28 + cosMap * 0.72;
}

function mapGridToRoundedRect(
  u: number,
  v: number,
  w: number,
  h: number,
  r: number,
): [number, number] {
  const x = (u - 0.5) * w;
  const y = (v - 0.5) * h;
  const hw = w / 2 - r;
  const hh = h / 2 - r;
  const ax = Math.abs(x);
  const ay = Math.abs(y);
  if (ax <= hw || ay <= hh) return [x, y];
  const dx = ax - hw;
  const dy = ay - hh;
  const dist = Math.hypot(dx, dy);
  if (dist <= r) return [x, y];
  const maxBox = Math.max(dx, dy);
  const scale = (r * (maxBox / r)) / dist;
  return [Math.sign(x) * (hw + dx * scale), Math.sign(y) * (hh + dy * scale)];
}

// ---------------------------------------------------------------------------
// Card wear profile & procedural RGBA mask + normal generation
// ---------------------------------------------------------------------------
export type CardWearProfile = {
  seed: number;
  creases: Array<{
    cx: number;
    cy: number;
    angle: number;
    halfLen: number;
    width: number;
    amp: number;
  }>;
  corners: Array<{
    sx: -1 | 1;
    sy: -1 | 1;
    size: number;
    amp: number;
    skew: number;
    wearWeight: number;
  }>;
  bowX: number;
  bowY: number;
  twist: number;
  stainX: number;
  stainY: number;
  stainR: number;
};

export function createCardWearProfile(seed: number): CardWearProfile {
  const rand = mulberry32(seed * 7919 + 13);
  const upperBias = rand() > 0.5 ? 0.68 : 0.3;
  const sideBias = rand() > 0.5 ? 0.72 : 0.28;
  const creases = [
    {
      cx: sideBias + (rand() - 0.5) * 0.14,
      cy: upperBias + (rand() - 0.5) * 0.08,
      angle: (sideBias > 0.5 ? 0.55 : -0.55) + (rand() - 0.5) * 0.32,
      halfLen: 0.36 + rand() * 0.12,
      width: 0.025 + rand() * 0.008,
      amp: (rand() > 0.5 ? 1 : -1) * (0.032 + rand() * 0.01),
    },
    {
      cx: 1 - sideBias + (rand() - 0.5) * 0.12,
      cy: 1 - upperBias + (rand() - 0.5) * 0.1,
      angle: (rand() - 0.5) * 0.45,
      halfLen: 0.28 + rand() * 0.1,
      width: 0.022 + rand() * 0.006,
      amp: (rand() > 0.5 ? 1 : -1) * (0.024 + rand() * 0.008),
    },
  ];
  const signs: Array<[-1 | 1, -1 | 1]> = [
    [1, 1],
    [-1, -1],
    [-1, 1],
    [1, -1],
  ];
  for (let i = signs.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    const tmp = signs[i]!;
    signs[i] = signs[j]!;
    signs[j] = tmp;
  }
  const corners = signs.map(([sx, sy], idx) => ({
    sx,
    sy,
    size:
      idx === 0
        ? 0.22 + rand() * 0.04
        : idx === 1
          ? 0.17 + rand() * 0.035
          : 0.12 + rand() * 0.03,
    amp:
      (idx % 2 === 0 ? 1 : -1) *
      (idx === 0
        ? 0.095 + rand() * 0.02
        : idx === 1
          ? 0.06 + rand() * 0.015
          : 0.028 + rand() * 0.01),
    skew: 0.88 + rand() * 0.24,
    wearWeight: idx === 0 ? 1.16 : idx === 1 ? 0.98 : 0.78 + rand() * 0.16,
  }));
  return {
    seed,
    creases,
    corners,
    bowX: (rand() > 0.5 ? 1 : -1) * (0.022 + rand() * 0.01),
    bowY: (rand() > 0.5 ? 1 : -1) * (0.016 + rand() * 0.008),
    twist: (rand() - 0.5) * 0.02,
    stainX: rand() > 0.5 ? 0.14 + rand() * 0.18 : 0.68 + rand() * 0.18,
    stainY: 0.15 + rand() * 0.7,
    stainR: 0.12 + rand() * 0.09,
  };
}

export type WearTexturePair = {
  maskTex: THREE.DataTexture;
  normalTex: THREE.DataTexture;
  dispose: () => void;
};

function buildCardWearBuffers(
  profile: CardWearProfile,
  mirrorX: boolean,
  width: number,
  height: number,
): { maskData: Uint8Array; heightField: Float32Array } {
  const maskData = new Uint8Array(width * height * 4);
  const heightField = new Float32Array(width * height);
  const noiseA = makeNoise2D(profile.seed * 31 + (mirrorX ? 997 : 17));
  const noiseB = makeNoise2D(profile.seed * 67 + 409);
  const noiseC = makeNoise2D(profile.seed * 113 + 733);
  const rand = mulberry32(profile.seed * 131 + 7);

  const scratches: Array<{
    x0: number;
    y0: number;
    dx: number;
    dy: number;
    len: number;
    strength: number;
    curve: number;
  }> = [];
  for (let i = 0; i < 20; i++) {
    const angle = (rand() > 0.28 ? Math.PI * 0.5 : 0) + (rand() - 0.5) * 0.85;
    scratches.push({
      x0: rand(),
      y0: rand(),
      dx: Math.cos(angle),
      dy: Math.sin(angle),
      len: 0.025 + rand() * 0.095,
      strength: 0.22 + rand() * 0.44,
      curve: (rand() - 0.5) * 0.012,
    });
  }

  for (let py = 0; py < height; py++) {
    const v = py / (height - 1);
    for (let px = 0; px < width; px++) {
      const uRaw = px / (width - 1);
      const u = mirrorX ? 1 - uRaw : uRaw;
      const idx = py * width + px;
      const minEdge = Math.min(u, 1 - u, v * 1.38, (1 - v) * 1.38);
      const nCoarse = noiseA.fbm(u * 7.5, v * 10.5, 4);
      const nMed = noiseB.fbm(u * 26.0 + 2.3, v * 36.0 + 4.1, 3);
      const nFine = noiseC.fbm(u * 88.0, v * 120.0, 2);

      let cornerWear = 0;
      for (const c of profile.corners) {
        const cd = Math.hypot(
          c.sx > 0 ? 1 - u : u,
          (c.sy > 0 ? 1 - v : v) * 1.38,
        );
        const reach = 0.14 * c.wearWeight;
        if (cd < reach) {
          const base = Math.pow(1 - cd / reach, 1.42);
          cornerWear = Math.max(
            cornerWear,
            base *
              (0.48 + nCoarse * 0.42 + nMed * 0.4 + nFine * 0.24) *
              c.wearWeight,
          );
        }
      }
      const edgeCluster = smoothstep(0.32, 0.78, nCoarse * 0.72 + nMed * 0.35);
      const maxBite = 0.048 * (0.38 + edgeCluster * 0.95);
      const edgeChip =
        Math.pow(Math.max(0, 1 - minEdge / Math.max(0.008, maxBite)), 1.48) *
        (0.36 + nMed * 0.52 + nFine * 0.4) *
        0.92;
      const microRim =
        (1 - smoothstep(0, 0.016, minEdge)) *
        smoothstep(0.34, 0.76, nMed * 0.6 + nFine * 0.55) *
        0.92;
      const edgeWear = Math.max(cornerWear, edgeChip, microRim);

      let creaseCrack = 0;
      for (const cr of profile.creases) {
        const dx = u - cr.cx;
        const dy = v - cr.cy;
        const ca = Math.cos(cr.angle);
        const sa = Math.sin(cr.angle);
        const along = dx * ca + dy * sa;
        const perp = -dx * sa + dy * ca;
        const lenMask =
          1 - smoothstep(cr.halfLen * 0.65, cr.halfLen, Math.abs(along));
        if (lenMask <= 0) continue;
        const wobble = (nMed - 0.5) * 0.011 + (nFine - 0.5) * 0.0055;
        const d = Math.abs(perp + wobble);
        const brk = smoothstep(0.24, 0.72, nMed * 0.55 + nFine * 0.65);
        creaseCrack = Math.max(
          creaseCrack,
          (Math.exp(-d / 0.0036) * (0.45 + brk * 0.85) +
            Math.exp(-d / 0.011) * 0.16 * brk) *
            lenMask,
        );
      }

      const rubZones =
        Math.exp(
          -Math.pow((u - 0.84) / 0.22, 2) - Math.pow((v - 0.36) / 0.32, 2),
        ) +
        Math.exp(
          -Math.pow((u - 0.16) / 0.22, 2) - Math.pow((v - 0.64) / 0.32, 2),
        );
      const scuff = Math.min(
        0.56,
        rubZones * (0.24 + nCoarse * 0.22 + nMed * 0.16) +
          smoothstep(0.45, 0.88, nMed * 0.65 + nFine * 0.35) * 0.2,
      );

      const perimGrime =
        (1 - smoothstep(0.01, 0.16, minEdge)) *
        (0.4 + nCoarse * 0.65 + nMed * 0.25);
      const sd = Math.hypot(u - profile.stainX, (v - profile.stainY) * 1.38);
      const stainRing =
        Math.exp(
          -Math.pow(
            (sd - profile.stainR * 0.86 + (nCoarse - 0.5) * 0.03) / 0.014,
            2,
          ),
        ) * 0.55;
      const grime = Math.min(1, perimGrime * 0.82 + stainRing + nCoarse * 0.16);

      maskData[idx * 4] = Math.min(255, Math.round(edgeWear * 255));
      maskData[idx * 4 + 1] = Math.min(255, Math.round(creaseCrack * 255));
      maskData[idx * 4 + 2] = Math.min(255, Math.round(scuff * 255));
      maskData[idx * 4 + 3] = Math.min(255, Math.round(grime * 255));
      heightField[idx] = (nFine - 0.5) * 0.05 - edgeWear * 0.25;
    }
  }

  for (const sc of scratches) {
    const steps = Math.max(12, Math.ceil(sc.len * width * 1.2));
    for (let s = 0; s <= steps; s++) {
      const t = s / steps;
      const bend = Math.sin(t * Math.PI) * sc.curve;
      const su = sc.x0 + sc.dx * sc.len * t - sc.dy * bend;
      const sv = sc.y0 + sc.dy * sc.len * t + sc.dx * bend;
      const cx = Math.round((mirrorX ? 1 - su : su) * (width - 1));
      const cy = Math.round(sv * (height - 1));
      if (cx < 1 || cx >= width - 1 || cy < 1 || cy >= height - 1) continue;
      const falloff = Math.sin(t * Math.PI) * sc.strength;
      const idx = cy * width + cx;
      maskData[idx * 4 + 2] = Math.min(
        255,
        Math.round(
          Math.max(maskData[idx * 4 + 2]! / 255, 0.58 + falloff * 0.42) * 255,
        ),
      );
      heightField[idx] = (heightField[idx] ?? 0) - falloff * 0.3;
    }
  }

  return { maskData, heightField };
}

function buildNormalData(
  heightField: Float32Array,
  width: number,
  height: number,
  gain: number,
): Uint8Array {
  const normalData = new Uint8Array(width * height * 4);
  for (let py = 0; py < height; py++) {
    const ym = Math.max(0, py - 1);
    const yp = Math.min(height - 1, py + 1);
    for (let px = 0; px < width; px++) {
      const xm = Math.max(0, px - 1);
      const xp = Math.min(width - 1, px + 1);
      const dx =
        (heightField[ym * width + xp]! +
          2 * heightField[py * width + xp]! +
          heightField[yp * width + xp]! -
          (heightField[ym * width + xm]! +
            2 * heightField[py * width + xm]! +
            heightField[yp * width + xm]!)) *
        gain;
      const dy =
        (heightField[yp * width + xm]! +
          2 * heightField[yp * width + px]! +
          heightField[yp * width + xp]! -
          (heightField[ym * width + xm]! +
            2 * heightField[ym * width + px]! +
            heightField[ym * width + xp]!)) *
        gain;
      const invLen = 1 / Math.hypot(dx, dy, 1);
      const idx = (py * width + px) * 4;
      normalData[idx] = Math.round((-dx * invLen * 0.5 + 0.5) * 255);
      normalData[idx + 1] = Math.round((-dy * invLen * 0.5 + 0.5) * 255);
      normalData[idx + 2] = Math.round((invLen * 0.5 + 0.5) * 255);
      normalData[idx + 3] = 255;
    }
  }
  return normalData;
}

export function createCardWearTextures(
  profile: CardWearProfile,
  mirrorX = false,
  width = 256,
  height = 360,
): WearTexturePair {
  const { maskData, heightField } = buildCardWearBuffers(
    profile,
    mirrorX,
    width,
    height,
  );
  const normalData = buildNormalData(heightField, width, height, 4.0);
  const maskTex = new THREE.DataTexture(
    maskData,
    width,
    height,
    THREE.RGBAFormat,
  );
  maskTex.colorSpace = THREE.NoColorSpace;
  maskTex.generateMipmaps = true;
  maskTex.minFilter = THREE.LinearMipmapLinearFilter;
  maskTex.needsUpdate = true;
  const normalTex = new THREE.DataTexture(
    normalData,
    width,
    height,
    THREE.RGBAFormat,
  );
  normalTex.colorSpace = THREE.NoColorSpace;
  normalTex.generateMipmaps = true;
  normalTex.minFilter = THREE.LinearMipmapLinearFilter;
  normalTex.needsUpdate = true;
  return {
    maskTex,
    normalTex,
    dispose: () => {
      maskTex.dispose();
      normalTex.dispose();
    },
  };
}

export type CardWearUniforms = {
  wearAmount: { value: number };
  bendAmount: { value: number };
  wearMask: { value: THREE.Texture };
};

export function patchCardFaceMaterial(
  material: THREE.MeshPhysicalMaterial,
  uniforms: CardWearUniforms,
  normalTex: THREE.Texture,
  wear: number,
  bend: number,
): void {
  const normalStrength = wear <= 0.001 && bend <= 0.001 ? 0 : wear * 0.22;
  material.normalMap = normalTex;
  material.normalScale.set(normalStrength, normalStrength);
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uWear = uniforms.wearAmount;
    shader.uniforms.uBend = uniforms.bendAmount;
    shader.uniforms.uMask = uniforms.wearMask;
    shader.vertexShader =
      `varying vec2 vWearUv;\n` +
      shader.vertexShader.replace(
        "#include <uv_vertex>",
        `#include <uv_vertex>\nvWearUv = uv;`,
      );
    shader.fragmentShader =
      `varying vec2 vWearUv; uniform float uWear; uniform float uBend; uniform sampler2D uMask;\n` +
      shader.fragmentShader;
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <color_fragment>",
      `#include <color_fragment>
if (uWear > 0.001 || uBend > 0.001) {
  float w = clamp(uWear, 0.0, 1.0);
  float b = clamp(uBend, 0.0, 1.0);
  vec4 wm = texture2D(uMask, vWearUv);
  float ageCurve = smoothstep(0.45, 1.0, w);
  float luma = dot(diffuseColor.rgb, vec3(0.299, 0.587, 0.114));
  diffuseColor.rgb = mix(diffuseColor.rgb, vec3(luma) * vec3(1.03, 0.99, 0.90), ageCurve * 0.16) * mix(vec3(1.0), vec3(1.01, 0.97, 0.88), ageCurve * 0.40);

  float grimeTh = mix(0.94, 0.28, smoothstep(0.54, 1.0, w));
  float grimeAmt = smoothstep(grimeTh, grimeTh + 0.40, wm.a) * smoothstep(0.52, 1.0, w);
  diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * 0.74 + vec3(0.05, 0.04, 0.03), grimeAmt * 0.46);

  float rubCloud = smoothstep(0.25, 0.60, wm.b) * smoothstep(0.58, 1.0, w);
  diffuseColor.rgb = mix(diffuseColor.rgb, mix(diffuseColor.rgb, vec3(0.74, 0.70, 0.63), 0.18), rubCloud * 0.34);

  float scratchTh = mix(0.90, 0.62, smoothstep(0.44, 1.0, w));
  float scratchLine = smoothstep(scratchTh, scratchTh + 0.22, wm.b) * smoothstep(0.42, 0.96, w);
  diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.78, 0.74, 0.66), scratchLine * (0.06 + smoothstep(0.55, 1.0, w) * 0.20));

  float edgeTh = mix(0.95, 0.15, pow(w, 1.05));
  float rawEdge = smoothstep(edgeTh, edgeTh + 0.12, wm.r) * smoothstep(0.14, 0.40, w);
  float haloEdge = smoothstep(edgeTh - 0.09, edgeTh + 0.03, wm.r) * (1.0 - rawEdge) * smoothstep(0.52, 0.92, w);
  diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * 0.68 + vec3(0.08, 0.07, 0.05), haloEdge * 0.44);
  vec3 core = mix(vec3(0.83, 0.79, 0.70), vec3(0.60, 0.53, 0.42), clamp(wm.a * 0.85 * smoothstep(0.55, 1.0, w), 0.0, 0.76));
  diffuseColor.rgb = mix(diffuseColor.rgb, core, rawEdge * mix(0.58, 0.90, smoothstep(0.32, 0.82, w)));

  float creaseDrive = smoothstep(0.36, 1.0, b);
  float creaseTh = mix(0.88, 0.24, creaseDrive);
  float crackAmt = smoothstep(creaseTh, creaseTh + 0.24, wm.g) * smoothstep(0.36, 0.92, b);
  diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.83, 0.79, 0.71), crackAmt * 0.62);
}`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <roughnessmap_fragment>",
      `#include <roughnessmap_fragment>
if (uWear > 0.001 || uBend > 0.001) {
  float w = clamp(uWear, 0.0, 1.0);
  float b = clamp(uBend, 0.0, 1.0);
  vec4 wm = texture2D(uMask, vWearUv);
  float dmg = max(wm.b * 0.85, max(wm.r, wm.g * smoothstep(0.36, 0.9, b)));
  roughnessFactor = mix(roughnessFactor, 0.95, clamp(smoothstep(0.24, 0.76, dmg * (0.28 + w * 0.92)) * w * 0.88, 0.0, 1.0));
}`,
    );
  };
  material.customProgramCacheKey = () => "sw-live-card-wear-v1";
}

// ---------------------------------------------------------------------------
// Sleeve wear profile, tight corner-tip hinges, stack nesting & shaders
// ---------------------------------------------------------------------------
export type SleeveCornerHinge = {
  sx: -1 | 1;
  sy: -1 | 1;
  nx: number;
  ny: number;
  arcX: number;
  arcY: number;
  maxFoldRad: number;
  hingeOffset: number;
  gate: number;
  weight: number;
};

export type SleeveWearProfile = {
  seed: number;
  corners: SleeveCornerHinge[];
  lipFreqA: number;
  lipFreqB: number;
  lipPhase: number;
  splitSide: -1 | 1;
  splitStartV: number;
  tearReach: number;
  tearSkew: number;
};

export function createSleeveWearProfile(seed: number): SleeveWearProfile {
  const rand = mulberry32(seed * 6133 + 29);
  const cornerOrder: Array<[-1 | 1, -1 | 1]> =
    rand() > 0.5
      ? [
          [1, 1],
          [-1, -1],
          [-1, 1],
          [1, -1],
        ]
      : [
          [-1, 1],
          [1, -1],
          [1, 1],
          [-1, -1],
        ];

  const corners: SleeveCornerHinge[] = cornerOrder.map(([sx, sy], idx) => {
    const diagAngle = Math.PI * 0.25 + (rand() - 0.5) * 0.16;
    const nx = sx * Math.cos(diagAngle);
    const ny = sy * Math.sin(diagAngle);
    const arcX = sx * (CARD_WIDTH / 2 - CARD_CORNER_RADIUS);
    const arcY = sy * (CARD_HEIGHT / 2 - CARD_CORNER_RADIUS) + CARD_OFFSET_Y;

    return {
      sx,
      sy,
      nx,
      ny,
      arcX,
      arcY,
      maxFoldRad:
        idx === 0
          ? 1.24 + rand() * 0.14
          : idx === 1
            ? 1.06 + rand() * 0.14
            : idx === 2
              ? 0.86 + rand() * 0.12
              : 0.66 + rand() * 0.12,
      // Tight corner-tip hinge placed just past the inner card corner.
      hingeOffset: 0.048 + rand() * 0.022,
      gate: idx === 0 ? 0.04 : idx === 1 ? 0.14 : idx === 2 ? 0.36 : 0.6,
      weight: idx === 0 ? 1.0 : idx === 1 ? 0.88 : idx === 2 ? 0.72 : 0.56,
    };
  });

  return {
    seed,
    corners,
    lipFreqA: 14 + rand() * 5,
    lipFreqB: 25 + rand() * 7,
    lipPhase: rand() * Math.PI * 2,
    splitSide: rand() > 0.5 ? 1 : -1,
    splitStartV: 0.46 + rand() * 0.16,
    tearReach: 0.21 + rand() * 0.05,
    tearSkew: 0.86 + rand() * 0.28,
  };
}

export function evalSleeveCornerFlapDist(
  px: number,
  py: number,
  c: SleeveCornerHinge,
): number {
  const dx = px - c.arcX;
  const dy = py - c.arcY;
  if (dx * c.sx < -0.02 || dy * c.sy < -0.02) return -1;
  return dx * c.nx + dy * c.ny - (CARD_CORNER_RADIUS + c.hingeOffset);
}

export function evalSleeveTearSignedDist(
  u: number,
  v: number,
  profile: SleeveWearProfile,
): number {
  const dx = profile.splitSide > 0 ? 1 - u : u;
  const dy = 1 - v;
  const diag = dx * profile.tearSkew + dy * (2.0 - profile.tearSkew) * 0.82;
  const jag =
    Math.sin(u * 38 + v * 29 + profile.seed) * 0.013 +
    Math.cos(u * 71 - v * 53 + profile.seed * 0.7) * 0.007;
  return profile.tearReach - (diag + jag);
}

function evalSleeveVertex(
  px: number,
  py: number,
  u: number,
  v: number,
  profile: SleeveWearProfile,
  bendStrength: number,
  lipWaveStrength: number,
  zSide: -1 | 1,
  onFaceDownDeck: boolean,
): [number, number, number] {
  let x = px;
  let y = py;
  let z = zSide * WORN_SLEEVE_HALF_THICK;

  if (bendStrength > 0.001) {
    for (const c of profile.corners) {
      const active = Math.pow(smoothstep(c.gate, 1.0, bendStrength), 0.92);
      if (active <= 0) continue;
      const dPast = evalSleeveCornerFlapDist(px, py, c);
      if (dPast <= 0) continue;
      const sharpRamp = smoothstep(0.0, 0.016, dPast);
      const theta = c.maxFoldRad * active * (onFaceDownDeck ? 0.78 : 1.0);
      const effD = dPast * (0.38 + 0.62 * sharpRamp);
      x -= c.nx * (1 - Math.cos(theta)) * effD;
      y -= c.ny * (1 - Math.cos(theta)) * effD;
      z += Math.sin(theta) * effD;
    }
  }

  if (lipWaveStrength > 0.001) {
    const topMask = Math.pow(smoothstep(0.84, 1.0, v), 1.45);
    if (topMask > 0) {
      const wave =
        Math.sin(u * profile.lipFreqA + profile.lipPhase) * 0.025 +
        Math.cos(u * profile.lipFreqB - profile.lipPhase * 0.6) * 0.012;
      if (onFaceDownDeck) {
        if (zSide < 0)
          z -= topMask * (0.012 + Math.max(-0.003, wave)) * lipWaveStrength;
        else
          z +=
            topMask * (0.003 + Math.max(-0.002, wave * 0.25)) * lipWaveStrength;
      } else {
        if (zSide > 0)
          z += topMask * (0.014 + Math.max(-0.004, wave)) * lipWaveStrength;
        else
          z +=
            topMask * (-0.006 + Math.min(0.004, wave * 0.45)) * lipWaveStrength;
      }
    }
  }

  if (bendStrength > 0.78) {
    const splitAmt = smoothstep(0.78, 1.0, bendStrength);
    const onSide = profile.splitSide > 0 ? 1 - u : u;
    const sideMask = Math.pow(
      1 - smoothstep(0.0, 0.12 + splitAmt * 0.12, onSide),
      1.45,
    );
    const splitFloor = profile.splitStartV + (1 - splitAmt) * 0.22;
    const vMask = smoothstep(splitFloor, Math.min(0.98, splitFloor + 0.16), v);
    const peel = sideMask * vMask * splitAmt;
    if (peel > 0.001) {
      if (onFaceDownDeck) {
        z += peel * (zSide < 0 ? -0.11 : 0.006);
        x += profile.splitSide * peel * (zSide < 0 ? 0.032 : 0.01);
      } else {
        z += peel * (zSide > 0 ? 0.12 : -0.065);
        x += profile.splitSide * peel * (zSide > 0 ? 0.038 : 0.018);
      }
    }
  }

  if (bendStrength > 0.92) {
    const tearAmt = smoothstep(0.92, 1.0, bendStrength);
    const td = evalSleeveTearSignedDist(u, v, profile);
    const nearTear =
      Math.exp(-Math.pow((td + 0.04) / 0.055, 2)) * smoothstep(-0.14, 0.02, td);
    if (nearTear > 0.001) {
      if (onFaceDownDeck) {
        z += nearTear * (zSide < 0 ? -0.14 : 0.008) * tearAmt;
        x -= profile.splitSide * nearTear * 0.034 * tearAmt;
        y -= nearTear * 0.034 * tearAmt;
      } else {
        z += nearTear * (zSide > 0 ? 0.14 : -0.075) * tearAmt;
        x -= profile.splitSide * nearTear * 0.038 * tearAmt;
        y -= nearTear * 0.038 * tearAmt;
      }
    }
  }

  return [x, y, z];
}

export function createSleeveWearTextures(
  profile: SleeveWearProfile,
  mirrorX = false,
  width = 256,
  height = 360,
): WearTexturePair {
  const maskData = new Uint8Array(width * height * 4);
  const heightField = new Float32Array(width * height);
  const noiseA = makeNoise2D(profile.seed * 29 + (mirrorX ? 701 : 11));
  const noiseB = makeNoise2D(profile.seed * 53 + 277);
  const rand = mulberry32(profile.seed * 97 + 19);

  const gunkDots: Array<{ x: number; y: number; r: number; strength: number }> =
    [];
  for (let i = 0; i < 48; i++) {
    const pick = rand();
    const gx =
      pick < 0.42
        ? 0.04 + rand() * 0.24
        : pick < 0.84
          ? 0.72 + rand() * 0.24
          : 0.14 + rand() * 0.72;
    const gy = pick < 0.84 ? 0.12 + rand() * 0.78 : 0.74 + rand() * 0.22;
    gunkDots.push({
      x: gx,
      y: gy,
      r: 0.003 + rand() * 0.0055,
      strength: 0.45 + rand() * 0.55,
    });
  }

  for (let py = 0; py < height; py++) {
    const v = py / (height - 1);
    const worldY = (v - 0.5) * WORN_SLEEVE_H;
    for (let px = 0; px < width; px++) {
      const uRaw = px / (width - 1);
      const u = mirrorX ? 1 - uRaw : uRaw;
      const worldX = (u - 0.5) * WORN_SLEEVE_W;
      const idx = py * width + px;
      const minSide = Math.min(u, 1 - u, v * 1.38);
      const nCoarse = noiseA.fbm(u * 5.5, v * 7.5, 4);
      const nMed = noiseB.fbm(u * 20.0, v * 28.0, 3);
      const nFine = noiseB.fbm(u * 68.0, v * 94.0, 2);

      let ridgeCrease = 0;
      let hVal = (nFine - 0.5) * 0.025;
      for (const c of profile.corners) {
        const dPast = evalSleeveCornerFlapDist(worldX, worldY, c);
        if (dPast > -0.1) {
          const distFromRidge = Math.abs(dPast - 0.008);
          ridgeCrease = Math.max(
            ridgeCrease,
            Math.exp(-distFromRidge / 0.012) * c.weight,
          );
          hVal += Math.exp(-distFromRidge / 0.018) * 0.85 * c.weight;
        }
      }

      const weldWear =
        (1 - smoothstep(0, 0.022, minSide)) *
        (0.32 + nCoarse * 0.42 + nMed * 0.25);
      const td = evalSleeveTearSignedDist(u, v, profile);
      const seamOrTear = Math.max(
        weldWear * 0.42,
        smoothstep(-0.04, 0.035, td),
      );

      const gripZones =
        Math.exp(
          -Math.pow((u - 0.82) / 0.28, 2) - Math.pow((v - 0.38) / 0.36, 2),
        ) +
        Math.exp(
          -Math.pow((u - 0.18) / 0.28, 2) - Math.pow((v - 0.62) / 0.36, 2),
        ) +
        Math.exp(
          -Math.pow((u - 0.5) / 0.4, 2) - Math.pow((v - 0.5) / 0.38, 2),
        ) *
          0.45;
      const filmHaze = Math.min(
        0.62,
        gripZones * (0.28 + nCoarse * 0.22 + nMed * 0.12) + nCoarse * 0.15,
      );

      const topDust = smoothstep(0.84, 1.0, v) * (0.45 + nCoarse * 0.5);
      const gripOil =
        gripZones * smoothstep(0.32, 0.76, nCoarse * 0.65 + nMed * 0.35) * 0.88;
      const edgeOil =
        (1 - smoothstep(0.01, 0.16, minSide)) * (0.3 + nCoarse * 0.55);
      let grime = Math.min(1, Math.max(topDust, gripOil, edgeOil));
      for (const gd of gunkDots) {
        const d = Math.hypot(u - gd.x, (v - gd.y) * 1.38);
        if (d < gd.r) {
          const sp = (1 - d / gd.r) * (0.55 + nFine * 0.45) * gd.strength;
          grime = Math.max(grime, 0.55 + sp * 0.45);
          hVal += sp * 0.2;
        }
      }

      maskData[idx * 4] = Math.min(255, Math.round(ridgeCrease * 255));
      maskData[idx * 4 + 1] = Math.min(255, Math.round(seamOrTear * 255));
      maskData[idx * 4 + 2] = Math.min(255, Math.round(filmHaze * 255));
      maskData[idx * 4 + 3] = Math.min(255, Math.round(grime * 255));
      heightField[idx] = hVal;
    }
  }

  const normalData = buildNormalData(heightField, width, height, 2.5);
  const maskTex = new THREE.DataTexture(
    maskData,
    width,
    height,
    THREE.RGBAFormat,
  );
  maskTex.colorSpace = THREE.NoColorSpace;
  maskTex.generateMipmaps = true;
  maskTex.minFilter = THREE.LinearMipmapLinearFilter;
  maskTex.needsUpdate = true;
  const normalTex = new THREE.DataTexture(
    normalData,
    width,
    height,
    THREE.RGBAFormat,
  );
  normalTex.colorSpace = THREE.NoColorSpace;
  normalTex.generateMipmaps = true;
  normalTex.minFilter = THREE.LinearMipmapLinearFilter;
  normalTex.needsUpdate = true;
  return {
    maskTex,
    normalTex,
    dispose: () => {
      maskTex.dispose();
      normalTex.dispose();
    },
  };
}

export type WornSleeveGeometries = {
  frontFilm: THREE.BufferGeometry;
  backFace: THREE.BufferGeometry;
  seam: THREE.BufferGeometry;
  dispose: () => void;
};

/** Build deformed sleeve meshes with tight inward-bent corners, lip wave, seam split, and tear. */
export function createWornSleeveGeometries(
  profile: SleeveWearProfile,
  bend: number,
  lipWave: number,
  onFaceDownDeck = false,
  segX = 44,
  segY = 58,
): WornSleeveGeometries {
  const cols = segX + 1;
  const rows = segY + 1;
  const count = cols * rows;
  const frontPos = new Float32Array(count * 3);
  const backPos = new Float32Array(count * 3);
  const frontUV = new Float32Array(count * 2);
  const backUV = new Float32Array(count * 2);

  for (let iy = 0; iy < rows; iy++) {
    const v = clusterEdge(iy / segY);
    for (let ix = 0; ix < cols; ix++) {
      const u = clusterEdge(ix / segX);
      const idx = iy * cols + ix;
      const [px, py] = mapGridToRoundedRect(
        u,
        v,
        WORN_SLEEVE_W,
        WORN_SLEEVE_H,
        WORN_SLEEVE_R,
      );
      const uPos = px / WORN_SLEEVE_W + 0.5;
      const vPos = py / WORN_SLEEVE_H + 0.5;
      const [fx, fy, fz] = evalSleeveVertex(
        px,
        py,
        uPos,
        vPos,
        profile,
        bend,
        lipWave,
        1,
        onFaceDownDeck,
      );
      const [bx, by, bz] = evalSleeveVertex(
        px,
        py,
        uPos,
        vPos,
        profile,
        bend,
        lipWave,
        -1,
        onFaceDownDeck,
      );
      frontPos[idx * 3] = fx;
      frontPos[idx * 3 + 1] = fy;
      frontPos[idx * 3 + 2] = fz;
      frontUV[idx * 2] = uPos;
      frontUV[idx * 2 + 1] = vPos;

      backPos[idx * 3] = bx;
      backPos[idx * 3 + 1] = by;
      backPos[idx * 3 + 2] = bz;
      backUV[idx * 2] = 1 - uPos;
      backUV[idx * 2 + 1] = vPos;
    }
  }

  const fIdx: number[] = [];
  const bIdx: number[] = [];
  for (let iy = 0; iy < segY; iy++) {
    for (let ix = 0; ix < segX; ix++) {
      const a = iy * cols + ix;
      fIdx.push(a, a + 1, a + cols + 1, a, a + cols + 1, a + cols);
      bIdx.push(a, a + cols + 1, a + 1, a, a + cols, a + cols + 1);
    }
  }

  const frontFilm = new THREE.BufferGeometry();
  frontFilm.setAttribute("position", new THREE.BufferAttribute(frontPos, 3));
  frontFilm.setAttribute("uv", new THREE.BufferAttribute(frontUV, 2));
  frontFilm.setIndex(fIdx);
  frontFilm.computeVertexNormals();

  const backFace = new THREE.BufferGeometry();
  backFace.setAttribute("position", new THREE.BufferAttribute(backPos, 3));
  backFace.setAttribute("uv", new THREE.BufferAttribute(backUV, 2));
  backFace.setIndex(bIdx);
  backFace.computeVertexNormals();

  const perim: Array<{ u: number; v: number }> = [];
  for (let ix = 0; ix <= segX; ix++)
    perim.push({ u: clusterEdge(ix / segX), v: 0 });
  for (let iy = 1; iy <= segY; iy++)
    perim.push({ u: 1, v: clusterEdge(iy / segY) });
  for (let ix = segX - 1; ix >= 0; ix--)
    perim.push({ u: clusterEdge(ix / segX), v: 1 });
  for (let iy = segY - 1; iy >= 0; iy--)
    perim.push({ u: 0, v: clusterEdge(iy / segY) });

  const sVerts = new Float32Array(perim.length * 6);
  for (let i = 0; i < perim.length; i++) {
    const pt = perim[i]!;
    const [px, py] = mapGridToRoundedRect(
      pt.u,
      pt.v,
      WORN_SLEEVE_W,
      WORN_SLEEVE_H,
      WORN_SLEEVE_R,
    );
    const uPos = px / WORN_SLEEVE_W + 0.5;
    const vPos = py / WORN_SLEEVE_H + 0.5;
    const [fx, fy, fz] = evalSleeveVertex(
      px,
      py,
      uPos,
      vPos,
      profile,
      bend,
      lipWave,
      1,
      onFaceDownDeck,
    );
    const [bx, by, bz] = evalSleeveVertex(
      px,
      py,
      uPos,
      vPos,
      profile,
      bend,
      lipWave,
      -1,
      onFaceDownDeck,
    );
    sVerts[i * 6] = fx;
    sVerts[i * 6 + 1] = fy;
    sVerts[i * 6 + 2] = fz;
    sVerts[i * 6 + 3] = bx;
    sVerts[i * 6 + 4] = by;
    sVerts[i * 6 + 5] = bz;
  }

  const splitAmt = smoothstep(0.78, 1.0, bend);
  const tearAmt = smoothstep(0.92, 1.0, bend);
  const splitFloor = profile.splitStartV + (1 - splitAmt) * 0.24;
  const activeIdx: number[] = [];
  for (let i = 0; i < perim.length - 1; i++) {
    const a = perim[i]!;
    const b = perim[i + 1]!;
    if (a.v > 0.995 && b.v > 0.995) continue;
    const midU = (a.u + b.u) * 0.5;
    const midV = (a.v + b.v) * 0.5;
    if (splitAmt > 0.02) {
      const onSplit = profile.splitSide > 0 ? midU > 0.985 : midU < 0.015;
      if (onSplit && midV >= splitFloor) continue;
    }
    if (tearAmt > 0.25 && evalSleeveTearSignedDist(midU, midV, profile) > 0)
      continue;
    const i0 = i * 2;
    activeIdx.push(i0, i0 + 1, i0 + 3, i0, i0 + 3, i0 + 2);
  }
  const seam = new THREE.BufferGeometry();
  seam.setAttribute("position", new THREE.BufferAttribute(sVerts, 3));
  seam.setIndex(activeIdx);
  seam.computeVertexNormals();

  return {
    frontFilm,
    backFace,
    seam,
    dispose: () => {
      frontFilm.dispose();
      backFace.dispose();
      seam.dispose();
    },
  };
}

/** Nested deformable plate for instanced 60-card library/graveyard stack layers. */
export function createNestedSleeveStackPlate(
  profile: SleeveWearProfile,
  bend: number,
  lipWave: number,
  depth = 0.017,
  segX = 20,
  segY = 26,
): THREE.BufferGeometry {
  const halfD = depth * 0.5;
  const cols = segX + 1;
  const rows = segY + 1;
  const gridCount = cols * rows;

  const perim: Array<{ u: number; v: number }> = [];
  for (let ix = 0; ix <= segX; ix++)
    perim.push({ u: clusterEdge(ix / segX), v: 0 });
  for (let iy = 1; iy <= segY; iy++)
    perim.push({ u: 1, v: clusterEdge(iy / segY) });
  for (let ix = segX - 1; ix >= 0; ix--)
    perim.push({ u: clusterEdge(ix / segX), v: 1 });
  for (let iy = segY - 1; iy >= 0; iy--)
    perim.push({ u: 0, v: clusterEdge(iy / segY) });

  const meta: Array<{
    px: number;
    py: number;
    u: number;
    v: number;
    zSign: -1 | 1;
  }> = [];
  const pushFace = (zSign: -1 | 1) => {
    for (let iy = 0; iy < rows; iy++) {
      const v = clusterEdge(iy / segY);
      for (let ix = 0; ix < cols; ix++) {
        const u = clusterEdge(ix / segX);
        const [px, py] = mapGridToRoundedRect(
          u,
          v,
          WORN_SLEEVE_W,
          WORN_SLEEVE_H,
          WORN_SLEEVE_R,
        );
        meta.push({
          px,
          py,
          u: px / WORN_SLEEVE_W + 0.5,
          v: py / WORN_SLEEVE_H + 0.5,
          zSign,
        });
      }
    }
  };
  pushFace(1);
  pushFace(-1);
  for (const pt of perim) {
    const [px, py] = mapGridToRoundedRect(
      pt.u,
      pt.v,
      WORN_SLEEVE_W,
      WORN_SLEEVE_H,
      WORN_SLEEVE_R,
    );
    const uPos = px / WORN_SLEEVE_W + 0.5;
    const vPos = py / WORN_SLEEVE_H + 0.5;
    meta.push({ px, py, u: uPos, v: vPos, zSign: 1 });
    meta.push({ px, py, u: uPos, v: vPos, zSign: -1 });
  }

  const pos = new Float32Array(meta.length * 3);
  for (let i = 0; i < meta.length; i++) {
    const m = meta[i]!;
    let x = m.px;
    let y = m.py;
    let z = m.zSign * halfD;
    if (bend > 0.001) {
      for (const c of profile.corners) {
        const active = Math.pow(smoothstep(c.gate, 1.0, bend), 0.92);
        if (active <= 0) continue;
        const dPast = evalSleeveCornerFlapDist(m.px, m.py, c);
        if (dPast <= 0) continue;
        const sharpRamp = smoothstep(0.0, 0.016, dPast);
        const theta = c.maxFoldRad * active * 0.78;
        const effD = dPast * (0.38 + 0.62 * sharpRamp);
        x -= c.nx * (1 - Math.cos(theta)) * effD;
        y -= c.ny * (1 - Math.cos(theta)) * effD;
        z += Math.sin(theta) * effD;
      }
    }
    if (lipWave > 0.001) {
      const topMask = Math.pow(smoothstep(0.84, 1.0, m.v), 1.45);
      if (topMask > 0) {
        const wave =
          Math.sin(m.u * profile.lipFreqA + profile.lipPhase) * 0.014 +
          Math.cos(m.u * profile.lipFreqB - profile.lipPhase * 0.6) * 0.007;
        z += topMask * wave * lipWave;
      }
    }
    pos[i * 3] = x;
    pos[i * 3 + 1] = y;
    pos[i * 3 + 2] = z;
  }

  const indices: number[] = [];
  for (let iy = 0; iy < segY; iy++) {
    for (let ix = 0; ix < segX; ix++) {
      const a = iy * cols + ix;
      indices.push(a, a + 1, a + cols + 1, a, a + cols + 1, a + cols);
      const b = gridCount + a;
      indices.push(b, b + cols + 1, b + 1, b, b + cols, b + cols + 1);
    }
  }
  const rimBase = gridCount * 2;
  for (let i = 0; i < perim.length - 1; i++) {
    const i0 = rimBase + i * 2;
    indices.push(i0, i0 + 1, i0 + 3, i0, i0 + 3, i0 + 2);
  }

  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  geo.setIndex(indices);
  geo.computeVertexNormals();
  return geo;
}

export type SleeveWearUniforms = {
  bend: { value: number };
  grime: { value: number };
  maskFront: { value: THREE.Texture };
  maskBack: { value: THREE.Texture };
};

export function patchSleeveFrontMaterial(
  material: THREE.MeshPhysicalMaterial,
  uniforms: SleeveWearUniforms,
  normalTex: THREE.Texture,
  bend: number,
  grime: number,
): void {
  const normalScale =
    bend <= 0.001 && grime <= 0.001 ? 0 : bend * 0.58 + grime * 0.2;
  material.normalMap = normalTex;
  material.normalScale.set(normalScale, normalScale);
  material.side = THREE.DoubleSide;
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uBend = uniforms.bend;
    shader.uniforms.uGrime = uniforms.grime;
    shader.uniforms.uMask = uniforms.maskFront;
    shader.vertexShader =
      `varying vec2 vWearUv;\n` +
      shader.vertexShader.replace(
        "#include <uv_vertex>",
        `#include <uv_vertex>\nvWearUv = uv;`,
      );
    shader.fragmentShader =
      `varying vec2 vWearUv; uniform float uBend; uniform float uGrime; uniform sampler2D uMask;\n` +
      shader.fragmentShader;
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <color_fragment>",
      `#include <color_fragment>
if (uBend > 0.001 || uGrime > 0.001) {
  float b = clamp(uBend, 0.0, 1.0);
  float w = clamp(uGrime, 0.0, 1.0);
  vec4 wm = texture2D(uMask, vWearUv);
  float tearActive = smoothstep(0.92, 0.99, b);
  if (tearActive > 0.1 && wm.g > mix(1.02, 0.56, tearActive)) discard;
  float tearRim = smoothstep(0.42, 0.56, wm.g) * tearActive;
  float ridgeGlint = smoothstep(0.32, 0.85, wm.r) * smoothstep(0.18, 0.92, b);
  float cloudHaze = smoothstep(0.14, 0.56, wm.b) * smoothstep(0.18, 1.0, w);
  float dirtyGunk = smoothstep(0.38, 0.88, wm.a) * smoothstep(0.32, 1.0, w);
  float gunkSpeck = smoothstep(0.72, 0.95, wm.a) * smoothstep(0.45, 1.0, w);
  vec3 tint = mix(vec3(0.92, 0.94, 0.97), vec3(0.42, 0.39, 0.34), dirtyGunk * 0.55 + gunkSpeck * 0.45);
  tint = mix(tint, vec3(0.96, 0.98, 1.0), max(ridgeGlint * 0.75, tearRim * 0.95));
  diffuseColor.rgb = tint;
  diffuseColor.a = clamp(0.04 + ridgeGlint * 0.28 + tearRim * 0.52 + cloudHaze * 0.20 + dirtyGunk * 0.13 + gunkSpeck * 0.16, 0.04, 0.72);
}`,
    );
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <roughnessmap_fragment>",
      `#include <roughnessmap_fragment>
if (uBend > 0.001 || uGrime > 0.001) {
  float w = clamp(uGrime, 0.0, 1.0);
  float b = clamp(uBend, 0.0, 1.0);
  vec4 wm = texture2D(uMask, vWearUv);
  roughnessFactor = mix(0.10, 0.74, clamp(wm.b * w * 0.95 + wm.r * b * 0.55 + wm.a * w * 0.4 + w * 0.2, 0.0, 1.0));
}`,
    );
  };
  material.customProgramCacheKey = () => "sw-live-sleeve-front-wear-v1";
}

export function patchSleeveBackMaterial(
  material: THREE.MeshPhysicalMaterial,
  uniforms: SleeveWearUniforms,
  normalTex: THREE.Texture,
  bend: number,
  grime: number,
): void {
  const normalScale =
    bend <= 0.001 && grime <= 0.001 ? 0 : bend * 0.64 + grime * 0.14;
  material.normalMap = normalTex;
  material.normalScale.set(normalScale, normalScale);
  material.side = THREE.DoubleSide;
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uBend = uniforms.bend;
    shader.uniforms.uGrime = uniforms.grime;
    shader.uniforms.uMask = uniforms.maskBack;
    shader.vertexShader =
      `varying vec2 vWearUv;\n` +
      shader.vertexShader.replace(
        "#include <uv_vertex>",
        `#include <uv_vertex>\nvWearUv = uv;`,
      );
    shader.fragmentShader =
      `varying vec2 vWearUv; uniform float uBend; uniform float uGrime; uniform sampler2D uMask;\n` +
      shader.fragmentShader;
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <color_fragment>",
      `#include <color_fragment>
if (uBend > 0.001 || uGrime > 0.001) {
  float b = clamp(uBend, 0.0, 1.0);
  float w = clamp(uGrime, 0.0, 1.0);
  vec4 wm = texture2D(uMask, vWearUv);
  float tearActive = smoothstep(0.92, 0.99, b);
  if (tearActive > 0.1 && wm.g > mix(1.02, 0.56, tearActive)) discard;
  float tearRim = smoothstep(0.42, 0.56, wm.g) * tearActive;
  float grimeAmt = smoothstep(0.28, 0.84, wm.a) * smoothstep(0.24, 1.0, w);
  float gunkSpeck = smoothstep(0.70, 0.94, wm.a) * smoothstep(0.36, 1.0, w);
  vec3 grimeTone = mix(diffuseColor.rgb * 0.55, vec3(0.11, 0.10, 0.09), 0.38);
  diffuseColor.rgb = mix(diffuseColor.rgb, grimeTone, grimeAmt * 0.48);
  diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.16, 0.14, 0.12), gunkSpeck * 0.58);
  float hingeLine = smoothstep(0.38, 0.88, wm.r) * smoothstep(0.24, 0.95, b);
  diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * 1.24 + vec3(0.055, 0.06, 0.07), hingeLine * 0.42);
  diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * 1.35 + vec3(0.26, 0.28, 0.31), tearRim * 0.72);
}`,
    );
  };
  material.customProgramCacheKey = () => "sw-live-sleeve-back-wear-v1";
}

// ---------------------------------------------------------------------------
// Fast deterministic 2D Canvas preview renderer for Cosmetics tiles & picker
// ---------------------------------------------------------------------------
function drawRoundedRectPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + w - r, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + r);
  ctx.lineTo(x + w, y + h - r);
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  ctx.lineTo(x + r, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}

/** Paint a deterministic 2D preview of a Card Effect (exact mask + seed as 3D). */
export function paintCardWearPreview(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  tier: WearTierId,
  seed?: number,
): void {
  const spec = cardWearTierSpec(tier);
  const w = spec.wear;
  const b = spec.bend;
  const effSeed = normalizeWearSeed(seed, spec.defaultSeed || 456);

  ctx.clearRect(0, 0, width, height);
  const pad = Math.round(Math.min(width, height) * 0.06);
  const cw = width - pad * 2;
  const ch = height - pad * 2;
  const cr = Math.max(4, Math.round(cw * 0.052));

  // Base card silhouette (classic dark border + warm art panel + rules box).
  ctx.save();
  drawRoundedRectPath(ctx, pad, pad, cw, ch, cr);
  ctx.clip();

  const borderGrad = ctx.createLinearGradient(pad, pad, pad + cw, pad + ch);
  borderGrad.addColorStop(0, "#191c22");
  borderGrad.addColorStop(1, "#0f1116");
  ctx.fillStyle = borderGrad;
  ctx.fillRect(pad, pad, cw, ch);

  const innerInset = Math.max(4, Math.round(cw * 0.068));
  const ix = pad + innerInset;
  const iy = pad + innerInset;
  const iw = cw - innerInset * 2;
  const ih = ch - innerInset * 2;

  const frameGrad = ctx.createLinearGradient(ix, iy, ix, iy + ih);
  frameGrad.addColorStop(0, "#8f3224");
  frameGrad.addColorStop(0.55, "#5d1f18");
  frameGrad.addColorStop(1, "#331412");
  ctx.fillStyle = frameGrad;
  drawRoundedRectPath(ctx, ix, iy, iw, ih, Math.max(2, cr * 0.55));
  ctx.fill();

  // Art window
  const artY = iy + ih * 0.11;
  const artH = ih * 0.43;
  const artGrad = ctx.createRadialGradient(
    ix + iw * 0.48,
    artY + artH * 0.38,
    iw * 0.04,
    ix + iw * 0.5,
    artY + artH * 0.5,
    iw * 0.68,
  );
  artGrad.addColorStop(0, "#f5be6b");
  artGrad.addColorStop(0.4, "#c94b2d");
  artGrad.addColorStop(1, "#2a121b");
  ctx.fillStyle = artGrad;
  ctx.fillRect(ix + iw * 0.06, artY, iw * 0.88, artH);

  // Text box
  const boxY = artY + artH + ih * 0.07;
  const boxH = ih * 0.31;
  ctx.fillStyle = "#e5d5bc";
  ctx.fillRect(ix + iw * 0.06, boxY, iw * 0.88, boxH);

  if (w > 0.001 || b > 0.001) {
    const profile = createCardWearProfile(effSeed);
    const img = ctx.getImageData(pad, pad, cw, ch);
    const data = img.data;
    const { maskData } = buildCardWearBuffers(profile, false, cw, ch);

    const ageCurve = smoothstep(0.45, 1.0, w);
    const grimeTh = 0.94 + (0.28 - 0.94) * smoothstep(0.54, 1.0, w);
    const grimeGate = smoothstep(0.52, 1.0, w);
    const rubGate = smoothstep(0.58, 1.0, w);
    const scratchTh = 0.9 + (0.62 - 0.9) * smoothstep(0.44, 1.0, w);
    const scratchGate = smoothstep(0.42, 0.96, w);
    const scratchStrength = 0.06 + smoothstep(0.55, 1.0, w) * 0.2;
    const edgeTh = 0.95 + (0.15 - 0.95) * Math.pow(w, 1.05);
    const edgeGate = smoothstep(0.14, 0.4, w);
    const haloGate = smoothstep(0.52, 0.92, w);
    const edgeMix = 0.58 + (0.9 - 0.58) * smoothstep(0.32, 0.82, w);
    const creaseDrive = smoothstep(0.36, 1.0, b);
    const creaseTh = 0.88 + (0.24 - 0.88) * creaseDrive;
    const creaseGate = smoothstep(0.36, 0.92, b);

    for (let py = 0; py < ch; py++) {
      // Canvas Y=0 is top, shader V=1 is top.
      const mRow = ch - 1 - py;
      for (let px = 0; px < cw; px++) {
        const cIdx = (py * cw + px) * 4;
        if (data[cIdx + 3] === 0) continue;
        const mIdx = (mRow * cw + px) * 4;
        const mR = maskData[mIdx]! / 255;
        const mG = maskData[mIdx + 1]! / 255;
        const mB = maskData[mIdx + 2]! / 255;
        const mA = maskData[mIdx + 3]! / 255;

        let r = data[cIdx]! / 255;
        let g = data[cIdx + 1]! / 255;
        let bl = data[cIdx + 2]! / 255;

        const luma = r * 0.299 + g * 0.587 + bl * 0.114;
        const desat = ageCurve * 0.16;
        r = (r + (luma * 1.03 - r) * desat) * (1 + 0.01 * ageCurve * 0.4);
        g = (g + (luma * 0.99 - g) * desat) * (1 - 0.03 * ageCurve * 0.4);
        bl = (bl + (luma * 0.9 - bl) * desat) * (1 - 0.12 * ageCurve * 0.4);

        const grimeAmt =
          smoothstep(grimeTh, grimeTh + 0.4, mA) * grimeGate * 0.46;
        if (grimeAmt > 0) {
          r += (r * 0.74 + 0.05 - r) * grimeAmt;
          g += (g * 0.74 + 0.04 - g) * grimeAmt;
          bl += (bl * 0.74 + 0.03 - bl) * grimeAmt;
        }

        const rubCloud = smoothstep(0.25, 0.6, mB) * rubGate * 0.34;
        if (rubCloud > 0) {
          r += (r * 0.82 + 0.74 * 0.18 - r) * rubCloud;
          g += (g * 0.82 + 0.7 * 0.18 - g) * rubCloud;
          bl += (bl * 0.82 + 0.63 * 0.18 - bl) * rubCloud;
        }

        const scratchLine =
          smoothstep(scratchTh, scratchTh + 0.22, mB) *
          scratchGate *
          scratchStrength;
        if (scratchLine > 0) {
          r += (0.78 - r) * scratchLine;
          g += (0.74 - g) * scratchLine;
          bl += (0.66 - bl) * scratchLine;
        }

        const rawEdge = smoothstep(edgeTh, edgeTh + 0.12, mR) * edgeGate;
        const haloEdge =
          smoothstep(edgeTh - 0.09, edgeTh + 0.03, mR) *
          (1 - rawEdge) *
          haloGate *
          0.44;
        if (haloEdge > 0) {
          r += (r * 0.68 + 0.08 - r) * haloEdge;
          g += (g * 0.68 + 0.07 - g) * haloEdge;
          bl += (bl * 0.68 + 0.05 - bl) * haloEdge;
        }
        if (rawEdge > 0) {
          const coreDark = Math.min(0.76, mA * 0.85 * smoothstep(0.55, 1.0, w));
          const crC = 0.83 + (0.6 - 0.83) * coreDark;
          const cgC = 0.79 + (0.53 - 0.79) * coreDark;
          const cbC = 0.7 + (0.42 - 0.7) * coreDark;
          const em = rawEdge * edgeMix;
          r += (crC - r) * em;
          g += (cgC - g) * em;
          bl += (cbC - bl) * em;
        }

        const crackAmt =
          smoothstep(creaseTh, creaseTh + 0.24, mG) * creaseGate * 0.62;
        if (crackAmt > 0) {
          r += (0.83 - r) * crackAmt;
          g += (0.79 - g) * crackAmt;
          bl += (0.71 - bl) * crackAmt;
        }

        data[cIdx] = Math.min(255, Math.max(0, Math.round(r * 255)));
        data[cIdx + 1] = Math.min(255, Math.max(0, Math.round(g * 255)));
        data[cIdx + 2] = Math.min(255, Math.max(0, Math.round(bl * 255)));
      }
    }
    ctx.putImageData(img, pad, pad);
  }
  ctx.restore();
}

/** Paint a deterministic 2D preview of a Sleeve Effect (tight corners, lip wave, grime, split/tear). */
export function paintSleeveWearPreview(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  tier: WearTierId,
  seed?: number,
): void {
  const spec = sleeveWearTierSpec(tier);
  const b = spec.cornerBend;
  const g = spec.grimeHaze;
  const lip = spec.lipWave;
  const effSeed = normalizeWearSeed(seed, spec.defaultSeed || 523);
  const profile = createSleeveWearProfile(effSeed);

  ctx.clearRect(0, 0, width, height);
  const pad = Math.round(Math.min(width, height) * 0.06);
  const sw = width - pad * 2;
  const sh = height - pad * 2;
  const sr = Math.max(4, Math.round(sw * 0.036));

  // Inner card corner peek (visible when Unplayable tears away a sleeve corner).
  if (b > 0.92) {
    const cardPadX = pad + Math.round(sw * 0.045);
    const cardPadY = pad + Math.round(sh * 0.04);
    const cw = sw - (cardPadX - pad) * 2;
    const ch = sh - (cardPadY - pad) * 2;
    ctx.save();
    drawRoundedRectPath(ctx, cardPadX, cardPadY, cw, ch, Math.max(3, sr));
    ctx.fillStyle = "#5c3b22";
    ctx.fill();
    ctx.strokeStyle = "#141414";
    ctx.lineWidth = Math.max(2, sw * 0.028);
    ctx.stroke();
    ctx.restore();
  }

  // Sleeve body painted pixel-by-pixel with exact tear discard, corner bend ridges, and grime.
  const tmp = document.createElement("canvas");
  tmp.width = sw;
  tmp.height = sh;
  const tctx = tmp.getContext("2d");
  if (!tctx) return;

  drawRoundedRectPath(tctx, 0, 0, sw, sh, sr);
  tctx.clip();

  const bg = tctx.createLinearGradient(0, 0, sw, sh);
  bg.addColorStop(0, "#1c2c4a");
  bg.addColorStop(0.55, "#111c32");
  bg.addColorStop(1, "#0a1121");
  tctx.fillStyle = bg;
  tctx.fillRect(0, 0, sw, sh);

  // Gold border inlay matching the Spellwake Midnight sleeve.
  const inset = Math.max(4, Math.round(sw * 0.075));
  drawRoundedRectPath(
    tctx,
    inset,
    inset,
    sw - inset * 2,
    sh - inset * 2,
    sr * 0.6,
  );
  tctx.strokeStyle = "#c8a76988";
  tctx.lineWidth = Math.max(1, sw * 0.012);
  tctx.stroke();

  // Stylized Spellwake mark in center.
  tctx.fillStyle = "#e6c37b";
  tctx.beginPath();
  tctx.ellipse(
    sw * 0.5,
    sh * 0.47,
    sw * 0.14,
    sh * 0.11,
    -0.35,
    0,
    Math.PI * 2,
  );
  tctx.fill();
  tctx.fillStyle = "#111c32";
  tctx.beginPath();
  tctx.ellipse(
    sw * 0.54,
    sh * 0.49,
    sw * 0.07,
    sh * 0.09,
    -0.35,
    0,
    Math.PI * 2,
  );
  tctx.fill();

  if (b > 0.001 || g > 0.001 || lip > 0.001) {
    const img = tctx.getImageData(0, 0, sw, sh);
    const data = img.data;
    const noiseA = makeNoise2D(profile.seed * 29 + 11);
    const noiseB = makeNoise2D(profile.seed * 53 + 277);
    const rand = mulberry32(profile.seed * 97 + 19);
    const gunkDots: Array<{
      x: number;
      y: number;
      r: number;
      strength: number;
    }> = [];
    for (let i = 0; i < 36; i++) {
      const pick = rand();
      gunkDots.push({
        x:
          pick < 0.42
            ? 0.04 + rand() * 0.24
            : pick < 0.84
              ? 0.72 + rand() * 0.24
              : 0.14 + rand() * 0.72,
        y: pick < 0.84 ? 0.12 + rand() * 0.78 : 0.74 + rand() * 0.22,
        r: 0.004 + rand() * 0.007,
        strength: 0.5 + rand() * 0.5,
      });
    }

    const tearActive = smoothstep(0.92, 0.99, b);
    const splitAmt = smoothstep(0.78, 1.0, b);
    const splitFloor = profile.splitStartV + (1 - splitAmt) * 0.22;

    for (let py = 0; py < sh; py++) {
      const v = 1 - py / (sh - 1);
      const worldY = (v - 0.5) * WORN_SLEEVE_H;
      for (let px = 0; px < sw; px++) {
        const u = px / (sw - 1);
        const worldX = (u - 0.5) * WORN_SLEEVE_W;
        const idx = (py * sw + px) * 4;
        if (data[idx + 3] === 0) continue;

        const td = evalSleeveTearSignedDist(u, v, profile);
        const tearMask = smoothstep(-0.04, 0.035, td);
        if (tearActive > 0.1 && tearMask > 1.02 + (0.56 - 1.02) * tearActive) {
          data[idx + 3] = 0;
          continue;
        }

        let r = data[idx]! / 255;
        let gr = data[idx + 1]! / 255;
        let bl = data[idx + 2]! / 255;

        // Top opening wave shading
        if (lip > 0.001 && v > 0.84) {
          const topMask = Math.pow(smoothstep(0.84, 1.0, v), 1.45);
          const wave =
            Math.sin(u * profile.lipFreqA + profile.lipPhase) * 0.5 +
            Math.cos(u * profile.lipFreqB - profile.lipPhase * 0.6) * 0.3;
          const hl = topMask * wave * lip * 0.16;
          r += hl;
          gr += hl;
          bl += hl * 1.1;
        }

        // Tight corner-tip hinge fold (darker bent tip + sharp bright crease ridge)
        for (const c of profile.corners) {
          const active = Math.pow(smoothstep(c.gate, 1.0, b), 0.92);
          if (active <= 0) continue;
          const dPast = evalSleeveCornerFlapDist(worldX, worldY, c);
          if (dPast > -0.04) {
            const ridge = Math.exp(-Math.abs(dPast - 0.006) / 0.015) * active;
            r += ridge * 0.32;
            gr += ridge * 0.35;
            bl += ridge * 0.4;
            if (dPast > 0.004) {
              const foldDark = smoothstep(0.004, 0.04, dPast) * active * 0.45;
              r *= 1 - foldDark;
              gr *= 1 - foldDark;
              bl *= 1 - foldDark;
            }
          }
        }

        // Matte grime & speckled shuffle gunk
        if (g > 0.001) {
          const nCoarse = noiseA.fbm(u * 5.5, v * 7.5, 3);
          const nMed = noiseB.fbm(u * 20, v * 28, 2);
          const gripZones =
            Math.exp(
              -Math.pow((u - 0.82) / 0.28, 2) - Math.pow((v - 0.38) / 0.36, 2),
            ) +
            Math.exp(
              -Math.pow((u - 0.18) / 0.28, 2) - Math.pow((v - 0.62) / 0.36, 2),
            );
          let grime =
            gripZones *
            smoothstep(0.32, 0.76, nCoarse * 0.65 + nMed * 0.35) *
            0.75;
          for (const gd of gunkDots) {
            const d = Math.hypot(u - gd.x, (v - gd.y) * 1.38);
            if (d < gd.r) {
              grime = Math.max(
                grime,
                0.62 + (1 - d / gd.r) * 0.38 * gd.strength,
              );
            }
          }
          const gAmt =
            smoothstep(0.28, 0.84, grime) * smoothstep(0.24, 1.0, g) * 0.5;
          r += (0.28 - r) * gAmt;
          gr += (0.26 - gr) * gAmt;
          bl += (0.23 - bl) * gAmt;
        }

        // Splitting side weld seam on Damaged / Unplayable
        if (splitAmt > 0.05 && v >= splitFloor) {
          const sideDist = profile.splitSide > 0 ? 1 - u : u;
          if (sideDist < 0.045) {
            const rim = (1 - sideDist / 0.045) * splitAmt * 0.65;
            r += rim * 0.55;
            gr += rim * 0.6;
            bl += rim * 0.65;
          }
        }

        // Furled whitish stretched-plastic rim around Unplayable tear
        const tearRim = smoothstep(0.42, 0.56, tearMask) * tearActive;
        if (tearRim > 0) {
          r += (0.86 - r) * tearRim * 0.85;
          gr += (0.89 - gr) * tearRim * 0.85;
          bl += (0.93 - bl) * tearRim * 0.85;
        }

        data[idx] = Math.min(255, Math.max(0, Math.round(r * 255)));
        data[idx + 1] = Math.min(255, Math.max(0, Math.round(gr * 255)));
        data[idx + 2] = Math.min(255, Math.max(0, Math.round(bl * 255)));
      }
    }
    tctx.putImageData(img, 0, 0);
  }

  ctx.drawImage(tmp, pad, pad);
}
