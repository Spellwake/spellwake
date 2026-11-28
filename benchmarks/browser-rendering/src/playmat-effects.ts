/**
 * Canonical 8-effect Playmat Effects catalog and deterministic procedural painters.
 *
 * Clean is the sole free default (zero overhead / bypassed). The seven paid effects
 * span $0.49 to $1.49 (49 to 149 cents) and accept a persisted pattern seed (100-999)
 * so players can randomize placement in Cosmetics and keep that exact look in matches.
 */

export type PlaymatEffectId =
  | "clean"
  | "dusty"
  | "oil_stained"
  | "frayed"
  | "creased"
  | "drink_spill"
  | "lgs_grime"
  | "scorched";

export type PlaymatEffectRarity = "common" | "uncommon" | "rare" | "mythic";

export type PlaymatEffectSpec = {
  readonly id: PlaymatEffectId;
  readonly canonicalId: string;
  readonly previewId: string;
  readonly assetRef: string;
  readonly name: string;
  readonly description: string;
  readonly isFree: boolean;
  readonly priceCents: 0 | 49 | 99 | 149;
  readonly priceLabel: string | null;
  readonly rarity: PlaymatEffectRarity;
  readonly defaultSeed: number;
};

export type PlaymatEffectSelection = {
  readonly id: PlaymatEffectId;
  readonly seed: number;
};

export const PLAYMAT_EFFECT_SEED_MIN = 100;
export const PLAYMAT_EFFECT_SEED_MAX = 999;

export const PLAYMAT_EFFECTS: readonly PlaymatEffectSpec[] = [
  {
    id: "clean",
    canonicalId: "e0000004-0001-4000-8000-000000000001",
    previewId: "preview-playmat_effect-clean",
    assetRef: "effect:playmat:clean",
    name: "Clean",
    description: "Fresh out of the tube with zero wear, stains, or creases.",
    isFree: true,
    priceCents: 0,
    priceLabel: null,
    rarity: "common",
    defaultSeed: 0,
  },
  {
    id: "dusty",
    canonicalId: "e0000004-0001-4000-8000-000000000002",
    previewId: "preview-playmat_effect-dusty",
    assetRef: "effect:playmat:dusty",
    name: "Dusty & Chalky",
    description:
      "Fine settled tabletop dust in outer corners with soft swept card-slide trails.",
    isFree: false,
    priceCents: 49,
    priceLabel: "$0.49",
    rarity: "common",
    defaultSeed: 214,
  },
  {
    id: "oil_stained",
    canonicalId: "e0000004-0001-4000-8000-000000000003",
    previewId: "preview-playmat_effect-oil_stained",
    assetRef: "effect:playmat:oil_stained",
    name: "Oil Stained",
    description:
      "Translucent snack and hand oil blots soaking darker into the playmat weave.",
    isFree: false,
    priceCents: 99,
    priceLabel: "$0.99",
    rarity: "common",
    defaultSeed: 347,
  },
  {
    id: "frayed",
    canonicalId: "e0000004-0001-4000-8000-000000000004",
    previewId: "preview-playmat_effect-frayed",
    assetRef: "effect:playmat:frayed",
    name: "Frayed & Fuzzy",
    description:
      "Unraveled edge threads, clustered fabric pilling fuzz, and worn forearm abrasion.",
    isFree: false,
    priceCents: 99,
    priceLabel: "$0.99",
    rarity: "uncommon",
    defaultSeed: 512,
  },
  {
    id: "creased",
    canonicalId: "e0000004-0001-4000-8000-000000000005",
    previewId: "preview-playmat_effect-creased",
    assetRef: "effect:playmat:creased",
    name: "Rolled & Creased",
    description:
      "Matte neoprene tube-roll waves, soft backpack quarter-fold memory, and lifted side curls.",
    isFree: false,
    priceCents: 99,
    priceLabel: "$0.99",
    rarity: "uncommon",
    defaultSeed: 641,
  },
  {
    id: "drink_spill",
    canonicalId: "e0000004-0001-4000-8000-000000000006",
    previewId: "preview-playmat_effect-drink_spill",
    assetRef: "effect:playmat:drink_spill",
    name: "Coffee Ring & Spill",
    description:
      "Dried mug ring with dark capillary rim, secondary ring, and cloth-soaked spill stain.",
    isFree: false,
    priceCents: 149,
    priceLabel: "$1.49",
    rarity: "rare",
    defaultSeed: 387,
  },
  {
    id: "lgs_grime",
    canonicalId: "e0000004-0001-4000-8000-000000000007",
    previewId: "preview-playmat_effect-lgs_grime",
    assetRef: "effect:playmat:lgs_grime",
    name: "LGS Veteran Grime",
    description:
      "Darkened wrist-rest patina along the bottom edge, handled corners, and bottom-left deck outline.",
    isFree: false,
    priceCents: 149,
    priceLabel: "$1.49",
    rarity: "rare",
    defaultSeed: 733,
  },
  {
    id: "scorched",
    canonicalId: "e0000004-0001-4000-8000-000000000008",
    previewId: "preview-playmat_effect-scorched",
    assetRef: "effect:playmat:scorched",
    name: "Ember Scorched",
    description:
      "Singed corner with toasted-amber heat halo, carbonized soot core, and stray ember burns.",
    isFree: false,
    priceCents: 149,
    priceLabel: "$1.49",
    rarity: "mythic",
    defaultSeed: 905,
  },
];

const EFFECT_BY_KEY = new Map<string, PlaymatEffectSpec>();
for (const fx of PLAYMAT_EFFECTS) {
  EFFECT_BY_KEY.set(fx.id, fx);
  EFFECT_BY_KEY.set(fx.canonicalId, fx);
  EFFECT_BY_KEY.set(fx.previewId, fx);
  EFFECT_BY_KEY.set(fx.assetRef, fx);
}

/** Resolve any effect ID, canonical UUID, preview ID, or asset ref. */
export function resolvePlaymatEffect(
  idOrRef: string | null | undefined,
): PlaymatEffectSpec | null {
  if (!idOrRef) return null;
  return EFFECT_BY_KEY.get(idOrRef) ?? null;
}

/** Return the spec for a known PlaymatEffectId (defaults cleanly to Clean). */
export function playmatEffectSpec(id: PlaymatEffectId): PlaymatEffectSpec {
  return EFFECT_BY_KEY.get(id) ?? PLAYMAT_EFFECTS[0]!;
}

/** Clamp a persisted seed into the valid deterministic range. */
export function normalizePlaymatEffectSeed(
  seed: number | null | undefined,
  fallback: number,
): number {
  if (typeof seed !== "number" || !Number.isFinite(seed)) return fallback;
  const rounded = Math.round(seed);
  if (rounded < PLAYMAT_EFFECT_SEED_MIN || rounded > PLAYMAT_EFFECT_SEED_MAX) {
    return fallback;
  }
  return rounded;
}

// ---------------------------------------------------------------------------
// Deterministic PRNG & Value Noise
// ---------------------------------------------------------------------------
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

function makeNoise(seed: number) {
  const r = mulberry32(seed);
  const p = new Uint8Array(512);
  const v = new Float32Array(256);
  for (let i = 0; i < 256; i++) {
    p[i] = i;
    v[i] = r();
  }
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(r() * (i + 1));
    const tmp = p[i] ?? 0;
    p[i] = p[j] ?? 0;
    p[j] = tmp;
  }
  for (let i = 0; i < 256; i++) p[i + 256] = p[i] ?? 0;
  const fade = (t: number) => t * t * t * (t * (t * 6 - 15) + 10);
  const sample = (x: number, y: number) => {
    const xi = Math.floor(x) & 255;
    const yi = Math.floor(y) & 255;
    const xf = x - Math.floor(x);
    const yf = y - Math.floor(y);
    const u = fade(xf);
    const w = fade(yf);
    const aa = v[p[((p[xi] ?? 0) + yi) & 511] ?? 0] ?? 0;
    const ba = v[p[((p[(xi + 1) & 255] ?? 0) + yi) & 511] ?? 0] ?? 0;
    const ab = v[p[((p[xi] ?? 0) + yi + 1) & 511] ?? 0] ?? 0;
    const bb = v[p[((p[(xi + 1) & 255] ?? 0) + yi + 1) & 511] ?? 0] ?? 0;
    return aa + (ba - aa) * u + (ab + (bb - ab) * u - (aa + (ba - aa) * u)) * w;
  };
  const fbm = (x: number, y: number, oct = 4) => {
    let s = 0;
    let a = 0.5;
    let f = 1;
    let norm = 0;
    for (let o = 0; o < oct; o++) {
      s += sample(x * f, y * f) * a;
      norm += a;
      a *= 0.5;
      f *= 2.02;
    }
    return s / norm;
  };
  return { sample, fbm };
}

function makeOffscreen(w: number, h: number): HTMLCanvasElement | null {
  if (typeof document === "undefined") return null;
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return c;
}

// ---------------------------------------------------------------------------
// 1) Dusty & Chalky
// ---------------------------------------------------------------------------
function paintDusty(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  seed: number,
  isDark: boolean,
): void {
  const rng = mulberry32(seed * 97 + 13);
  const n = makeNoise(seed * 31 + 7);
  const layer = makeOffscreen(w, h);
  const small = makeOffscreen(240, 150);
  if (!layer || !small) return;
  const lctx = layer.getContext("2d");
  const sctx = small.getContext("2d");
  if (!lctx || !sctx) return;

  const sw = 240;
  const sh = 150;
  const img = sctx.createImageData(sw, sh);
  const d = img.data;
  const maxAlpha = isDark ? 0.25 : 0.3;
  for (let py = 0; py < sh; py++) {
    const v = py / (sh - 1);
    for (let px = 0; px < sw; px++) {
      const u = px / (sw - 1);
      const edgeDist = Math.hypot((u - 0.5) * 1.48, (v - 0.52) * 1.56);
      const macro = n.fbm(u * 4.2 + 1.3, v * 3.1 + 2.7, 3);
      const powder = n.fbm(u * 28, v * 19, 3);
      const grain = (n.sample(px * 0.9, py * 0.9) - 0.5) * 0.14;
      const base = Math.max(0, edgeDist * 0.44 + macro * 0.28 - 0.34);
      const alpha = Math.min(
        maxAlpha,
        Math.max(0, base * (0.68 + powder * 0.65) + grain * base),
      );
      const idx = (py * sw + px) * 4;
      d[idx] = 220;
      d[idx + 1] = 215;
      d[idx + 2] = 204;
      d[idx + 3] = Math.round(alpha * 255);
    }
  }
  sctx.putImageData(img, 0, 0);
  lctx.drawImage(small, 0, 0, w, h);

  for (let i = 0; i < 620; i++) {
    const x = rng() * w;
    const y = rng() * h;
    const ed = Math.hypot((x / w - 0.5) * 1.6, (y / h - 0.5) * 1.6);
    if (rng() > 0.25 + ed * 0.72) continue;
    const r = 0.45 + rng() * 1.25;
    lctx.fillStyle = `rgba(232, 228, 218, ${(0.18 + rng() * 0.36).toFixed(2)})`;
    lctx.beginPath();
    lctx.arc(x, y, r, 0, Math.PI * 2);
    lctx.fill();
  }
  lctx.lineCap = "round";
  for (let i = 0; i < 42; i++) {
    const x = rng() * w;
    const y = rng() * h;
    const ang = rng() * Math.PI * 2;
    const len = 4 + rng() * 10;
    lctx.strokeStyle = `rgba(228, 224, 214, ${(0.22 + rng() * 0.26).toFixed(2)})`;
    lctx.lineWidth = 0.75 + rng() * 0.45;
    lctx.beginPath();
    lctx.moveTo(x, y);
    lctx.quadraticCurveTo(
      x + Math.cos(ang + 0.5) * len * 0.5,
      y + Math.sin(ang + 0.5) * len * 0.5,
      x + Math.cos(ang) * len,
      y + Math.sin(ang) * len,
    );
    lctx.stroke();
  }

  lctx.save();
  lctx.globalCompositeOperation = "destination-out";
  for (let i = 0; i < 4; i++) {
    const x0 = w * (0.16 + rng() * 0.5);
    const y0 = h * (0.28 + rng() * 0.44);
    const ang = (rng() - 0.5) * 0.55;
    const len = w * (0.14 + rng() * 0.18);
    const steps = 18;
    for (let s = 0; s <= steps; s++) {
      const t = s / steps;
      const sx = x0 + Math.cos(ang) * len * t + Math.sin(t * Math.PI) * 14;
      const sy = y0 + Math.sin(ang) * len * t + Math.sin(t * Math.PI) * 10;
      const rad = 28 + Math.sin(t * Math.PI) * 26;
      const rg = lctx.createRadialGradient(sx, sy, 2, sx, sy, rad);
      rg.addColorStop(0, "rgba(0,0,0,0.22)");
      rg.addColorStop(0.65, "rgba(0,0,0,0.09)");
      rg.addColorStop(1, "rgba(0,0,0,0)");
      lctx.fillStyle = rg;
      lctx.beginPath();
      lctx.arc(sx, sy, rad, 0, Math.PI * 2);
      lctx.fill();
    }
  }
  lctx.restore();

  ctx.drawImage(layer, 0, 0);
}

// ---------------------------------------------------------------------------
// 2) Oil Stained (smooth absorbed grease/oil blots, zero crumbs)
// ---------------------------------------------------------------------------
function paintOilStained(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  seed: number,
  isDark: boolean,
): void {
  const rng = mulberry32(seed * 151 + 23);
  const blotCount = 8;

  for (let i = 0; i < blotCount; i++) {
    const gx = w * (0.1 + rng() * 0.8);
    const gy = h * (0.12 + rng() * 0.76);
    const gr = (i < 2 ? 36 : 18) + rng() * (i < 2 ? 32 : 24);
    const ph1 = rng() * 6.28;
    const ph2 = rng() * 6.28;
    const aspect = 0.72 + rng() * 0.28;
    const rot = rng() * Math.PI;

    const traceBlot = (c: CanvasRenderingContext2D, scaleR = 1) => {
      c.beginPath();
      for (let k = 0; k <= 36; k++) {
        const a = (k / 36) * Math.PI * 2;
        const rr =
          gr *
          scaleR *
          (0.8 + 0.14 * Math.sin(a * 3 + ph1) + 0.08 * Math.cos(a * 5 + ph2));
        const x = Math.cos(a) * rr;
        const y = Math.sin(a) * rr * aspect;
        if (k === 0) c.moveTo(x, y);
        else c.lineTo(x, y);
      }
      c.closePath();
    };

    ctx.save();
    ctx.translate(gx, gy);
    ctx.rotate(rot);
    if ("filter" in ctx) ctx.filter = "blur(2.4px)";
    ctx.globalCompositeOperation = "multiply";
    const grad = ctx.createRadialGradient(0, 0, gr * 0.08, 0, 0, gr);
    grad.addColorStop(0, "rgba(104, 90, 64, 0.56)");
    grad.addColorStop(0.68, "rgba(110, 94, 68, 0.42)");
    grad.addColorStop(0.92, "rgba(128, 110, 82, 0.18)");
    grad.addColorStop(1, "rgba(128, 110, 82, 0)");
    ctx.fillStyle = grad;
    traceBlot(ctx, 1.0);
    ctx.fill();
    ctx.restore();

    // Subtle matte oily sheen rim so oil blots also read physically on near-black mats
    if (isDark) {
      ctx.save();
      ctx.translate(gx, gy);
      ctx.rotate(rot);
      if ("filter" in ctx) ctx.filter = "blur(2.8px)";
      const sheen = ctx.createRadialGradient(
        -gr * 0.18,
        -gr * 0.18,
        gr * 0.05,
        0,
        0,
        gr,
      );
      sheen.addColorStop(0, "rgba(148, 134, 106, 0.14)");
      sheen.addColorStop(0.65, "rgba(96, 86, 68, 0.08)");
      sheen.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = sheen;
      traceBlot(ctx, 0.96);
      ctx.fill();
      ctx.restore();
    }
  }
}

// ---------------------------------------------------------------------------
// 3) Coffee Ring & Spill
// ---------------------------------------------------------------------------
function drawCoffeeRing(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  radius: number,
  rng: () => number,
  intensity: number,
  isBroken: boolean,
  isDark: boolean,
): void {
  const wobA = rng() * 6.28;
  const wobB = rng() * 6.28;

  const interior = ctx.createRadialGradient(
    cx,
    cy,
    radius * 0.08,
    cx,
    cy,
    radius * 1.1,
  );
  interior.addColorStop(
    0,
    `rgba(116, 68, 28, ${(0.07 * intensity).toFixed(3)})`,
  );
  interior.addColorStop(
    0.74,
    `rgba(98, 54, 20, ${(0.14 * intensity).toFixed(3)})`,
  );
  interior.addColorStop(
    0.93,
    `rgba(68, 34, 11, ${(0.34 * intensity).toFixed(3)})`,
  );
  interior.addColorStop(1, "rgba(68, 34, 11, 0)");

  ctx.save();
  ctx.globalCompositeOperation = "multiply";
  ctx.fillStyle = interior;
  ctx.beginPath();
  ctx.arc(cx, cy, radius * 1.1, 0, Math.PI * 2);
  ctx.fill();

  const segs = 128;
  const gapStart = rng() * Math.PI * 2;
  const gapSpan = isBroken ? 0.9 + rng() * 1.15 : 0;

  for (let pass = 0; pass < 3; pass++) {
    const rOff = (pass - 1) * 1.8;
    const baseW = pass === 1 ? 2.8 : 4.2;
    const baseA = (pass === 1 ? 0.42 : 0.16) * intensity;

    for (let i = 0; i < segs; i++) {
      const a0 = (i / segs) * Math.PI * 2;
      const a1 = ((i + 1.3) / segs) * Math.PI * 2;
      if (gapSpan > 0) {
        const diff =
          ((a0 - gapStart + Math.PI * 3) % (Math.PI * 2)) - Math.PI;
        if (Math.abs(diff) < gapSpan * 0.5) continue;
      }
      const w0 =
        Math.sin(a0 * 3 + wobA) * 1.9 + Math.cos(a0 * 7 + wobB) * 0.85;
      const w1 =
        Math.sin(a1 * 3 + wobA) * 1.9 + Math.cos(a1 * 7 + wobB) * 0.85;
      const mod =
        0.45 + 0.85 * Math.pow(0.5 + 0.5 * Math.sin(a0 * 2 + wobB), 1.3);
      ctx.strokeStyle = `rgba(70, 34, 11, ${(baseA * mod).toFixed(3)})`;
      ctx.lineWidth = baseW * mod;
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(
        cx + Math.cos(a0) * (radius + rOff + w0),
        cy + Math.sin(a0) * (radius + rOff + w0),
      );
      ctx.lineTo(
        cx + Math.cos(a1) * (radius + rOff + w1),
        cy + Math.sin(a1) * (radius + rOff + w1),
      );
      ctx.stroke();
    }
  }

  for (let i = 0; i < 16; i++) {
    const ang = rng() * Math.PI * 2;
    const dist = radius * (1.1 + rng() * 0.82);
    const dx = cx + Math.cos(ang) * dist;
    const dy = cy + Math.sin(ang) * dist;
    const dr = 1.6 + rng() * 5.8;
    const dg = ctx.createRadialGradient(dx, dy, dr * 0.15, dx, dy, dr);
    dg.addColorStop(
      0,
      `rgba(104, 58, 20, ${(0.16 * intensity).toFixed(3)})`,
    );
    dg.addColorStop(
      0.75,
      `rgba(74, 36, 12, ${(0.34 * intensity).toFixed(3)})`,
    );
    dg.addColorStop(1, "rgba(74, 36, 12, 0)");
    ctx.fillStyle = dg;
    ctx.beginPath();
    ctx.arc(dx, dy, dr, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();

  ctx.save();
  ctx.strokeStyle = isDark
    ? `rgba(168, 124, 76, ${(0.34 * intensity).toFixed(3)})`
    : `rgba(122, 72, 30, ${(0.18 * intensity).toFixed(3)})`;
  ctx.lineWidth = isDark ? 2.6 : 1.8;
  if ("filter" in ctx) ctx.filter = "blur(0.7px)";
  ctx.beginPath();
  for (let i = 0; i <= 96; i++) {
    const a = (i / 96) * Math.PI * 2;
    const rr =
      radius + Math.sin(a * 3 + wobA) * 1.9 + Math.cos(a * 7 + wobB) * 0.85;
    const x = cx + Math.cos(a) * rr;
    const y = cy + Math.sin(a) * rr;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
  ctx.restore();
}

function paintDrinkSpill(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  seed: number,
  isDark: boolean,
): void {
  const rng = mulberry32(seed * 131 + 19);
  const rightSide = rng() > 0.42;
  const cx1 = w * (rightSide ? 0.73 + rng() * 0.11 : 0.15 + rng() * 0.12);
  const cy1 = h * (0.21 + rng() * 0.2);
  const r1 = 54 + rng() * 14;
  drawCoffeeRing(ctx, cx1, cy1, r1, rng, 1.0, false, isDark);

  const cx2 = cx1 + (rightSide ? -1 : 1) * (24 + rng() * 26);
  const cy2 = cy1 + 18 + rng() * 22;
  drawCoffeeRing(
    ctx,
    cx2,
    cy2,
    r1 * (0.94 + rng() * 0.08),
    rng,
    0.54,
    true,
    isDark,
  );

  const sx = w * (rightSide ? 0.79 + rng() * 0.1 : 0.13 + rng() * 0.12);
  const sy = h * (0.74 + rng() * 0.13);
  const sr = 58 + rng() * 32;
  const ph1 = rng() * 6.28;
  const ph2 = rng() * 6.28;
  const tracePuddle = (c: CanvasRenderingContext2D, scaleR = 1) => {
    c.beginPath();
    for (let i = 0; i <= 40; i++) {
      const a = (i / 40) * Math.PI * 2;
      const rr =
        sr *
        scaleR *
        (0.78 + 0.22 * Math.sin(a * 3 + ph1) + 0.13 * Math.cos(a * 5 + ph2));
      const px = sx + Math.cos(a) * rr * 1.24;
      const py = sy + Math.sin(a) * rr * 0.82;
      if (i === 0) c.moveTo(px, py);
      else c.lineTo(px, py);
    }
    c.closePath();
  };

  ctx.save();
  if ("filter" in ctx) ctx.filter = "blur(2.2px)";
  ctx.globalCompositeOperation = "multiply";
  tracePuddle(ctx, 1.0);
  const puddle = ctx.createRadialGradient(sx, sy, sr * 0.1, sx, sy, sr * 1.24);
  puddle.addColorStop(0, "rgba(112, 68, 28, 0.18)");
  puddle.addColorStop(0.76, "rgba(86, 46, 16, 0.28)");
  puddle.addColorStop(0.95, "rgba(64, 30, 10, 0.36)");
  puddle.addColorStop(1, "rgba(64, 30, 10, 0)");
  ctx.fillStyle = puddle;
  ctx.fill();
  ctx.restore();

  if (isDark) {
    ctx.save();
    if ("filter" in ctx) ctx.filter = "blur(2px)";
    tracePuddle(ctx, 0.98);
    ctx.strokeStyle = "rgba(164, 122, 74, 0.22)";
    ctx.lineWidth = 3.2;
    ctx.stroke();
    ctx.restore();
  }
}

// ---------------------------------------------------------------------------
// 4) Frayed & Fuzzy
// ---------------------------------------------------------------------------
function paintFrayed(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  seed: number,
): void {
  const rng = mulberry32(seed * 73 + 5);
  const n = makeNoise(seed * 23 + 11);
  const small = makeOffscreen(240, 150);
  if (!small) return;
  const sctx = small.getContext("2d");
  if (!sctx) return;

  const sw = 240;
  const sh = 150;
  const img = sctx.createImageData(sw, sh);
  const d = img.data;
  for (let py = 0; py < sh; py++) {
    const v = py / (sh - 1);
    for (let px = 0; px < sw; px++) {
      const u = px / (sw - 1);
      const bottomRub = Math.exp(-Math.pow((v - 0.95) / 0.22, 2));
      const sideRub = Math.exp(-Math.pow((u - 0.97) / 0.1, 2)) * 0.6;
      const fuzzy = n.fbm(u * 24, v * 16, 3);
      const a = Math.min(
        0.28,
        (bottomRub * 0.22 + sideRub * 0.18) * (0.5 + fuzzy),
      );
      const idx = (py * sw + px) * 4;
      d[idx] = 224;
      d[idx + 1] = 220;
      d[idx + 2] = 212;
      d[idx + 3] = Math.round(a * 255);
    }
  }
  sctx.putImageData(img, 0, 0);
  ctx.drawImage(small, 0, 0, w, h);

  const pillZones = [
    { cx: w * 0.26, cy: h * 0.87, rx: 140, ry: 44, n: 120 },
    { cx: w * 0.7, cy: h * 0.88, rx: 150, ry: 42, n: 125 },
    { cx: w * 0.48, cy: h * 0.68, rx: 160, ry: 55, n: 75 },
    { cx: w * 0.89, cy: h * 0.56, rx: 45, ry: 110, n: 65 },
  ];
  for (const z of pillZones) {
    for (let i = 0; i < z.n; i++) {
      const ang = rng() * Math.PI * 2;
      const rad = Math.pow(rng(), 0.72);
      const x = Math.max(
        12,
        Math.min(w - 12, z.cx + Math.cos(ang) * z.rx * rad),
      );
      const y = Math.max(
        12,
        Math.min(h - 12, z.cy + Math.sin(ang) * z.ry * rad),
      );
      const r = 0.75 + rng() * 1.75;
      ctx.fillStyle = "rgba(0, 0, 0, 0.28)";
      ctx.beginPath();
      ctx.arc(x + 0.7, y + 0.9, r * 1.1, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = `rgba(214, 210, 202, ${(0.28 + rng() * 0.34).toFixed(2)})`;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  const frayZones = [
    { side: "bottom", t0: 0.14 + rng() * 0.14, span: 0.2 },
    { side: "bottom", t0: 0.56 + rng() * 0.16, span: 0.22 },
    { side: "right", t0: 0.34 + rng() * 0.26, span: 0.24 },
    { side: "left", t0: 0.58 + rng() * 0.18, span: 0.18 },
  ] as const;

  for (const fz of frayZones) {
    ctx.save();
    if ("filter" in ctx) ctx.filter = "blur(1.4px)";
    ctx.strokeStyle = "rgba(22, 20, 18, 0.65)";
    ctx.lineWidth = 7;
    ctx.lineCap = "round";
    ctx.beginPath();
    if (fz.side === "bottom") {
      ctx.moveTo(w * fz.t0, h - 5);
      ctx.lineTo(w * (fz.t0 + fz.span), h - 5);
    } else if (fz.side === "right") {
      ctx.moveTo(w - 5, h * fz.t0);
      ctx.lineTo(w - 5, h * (fz.t0 + fz.span));
    } else {
      ctx.moveTo(5, h * fz.t0);
      ctx.lineTo(5, h * (fz.t0 + fz.span));
    }
    ctx.stroke();
    ctx.restore();

    for (let i = 0; i < 52; i++) {
      const frac = fz.t0 + rng() * fz.span;
      let sx = 0;
      let sy = 0;
      let baseAng = 0;
      if (fz.side === "bottom") {
        sx = w * frac;
        sy = h - 3 - rng() * 4;
        baseAng = -Math.PI / 2 + (rng() - 0.5) * 2.2;
      } else if (fz.side === "right") {
        sx = w - 3 - rng() * 4;
        sy = h * frac;
        baseAng = Math.PI + (rng() - 0.5) * 2.2;
      } else {
        sx = 3 + rng() * 4;
        sy = h * frac;
        baseAng = (rng() - 0.5) * 2.2;
      }
      const len = 5 + rng() * 17;
      const curve = (rng() - 0.5) * 18;
      const ex = sx + Math.cos(baseAng) * len;
      const ey = sy + Math.sin(baseAng) * len;
      const c1x = sx + Math.cos(baseAng + 0.5) * len * 0.5 + curve * 0.4;
      const c1y = sy + Math.sin(baseAng + 0.5) * len * 0.5 + curve * 0.4;
      const isLoop = rng() > 0.72;

      ctx.lineCap = "round";
      ctx.strokeStyle = "rgba(0,0,0,0.38)";
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.moveTo(sx, sy);
      if (isLoop) {
        ctx.bezierCurveTo(
          c1x + 1,
          c1y + 1,
          ex + 1,
          ey + 1,
          sx + (rng() - 0.5) * 8 + 1,
          sy + 1,
        );
      } else {
        ctx.quadraticCurveTo(c1x + 1, c1y + 1, ex + 1, ey + 1);
      }
      ctx.stroke();

      const tone = Math.round(168 + rng() * 72);
      ctx.strokeStyle = `rgba(${tone}, ${tone - 5}, ${tone - 12}, ${(0.42 + rng() * 0.44).toFixed(2)})`;
      ctx.lineWidth = 0.7 + rng() * 0.7;
      ctx.beginPath();
      ctx.moveTo(sx, sy);
      if (isLoop) {
        ctx.bezierCurveTo(c1x, c1y, ex, ey, sx + (rng() - 0.5) * 8, sy);
      } else {
        ctx.quadraticCurveTo(c1x, c1y, ex, ey);
      }
      ctx.stroke();
    }
  }
}

// ---------------------------------------------------------------------------
// 5) Rolled & Creased (matte neoprene tube waves & soft backpack fold memory)
// ---------------------------------------------------------------------------
function paintCreased(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  seed: number,
): void {
  const rng = mulberry32(seed * 53 + 29);
  const sw = 320;
  const sh = 200;
  const small = makeOffscreen(sw, sh);
  if (!small) return;
  const sctx = small.getContext("2d");
  if (!sctx) return;

  const img = sctx.createImageData(sw, sh);
  const d = img.data;
  const foldX = 0.48 + (rng() - 0.5) * 0.08;
  const foldY = 0.5 + (rng() - 0.5) * 0.08;
  const rollFreq = 2.6 + rng() * 1.1;
  const rollPhase = rng() * 6.28;
  const diagShift = (rng() - 0.5) * 0.14;

  const heightAt = (u: number, v: number): number => {
    const edgeCurlL = Math.pow(Math.max(0, 0.19 - u) / 0.19, 2.2) * 0.95;
    const edgeCurlR = Math.pow(Math.max(0, u - 0.81) / 0.19, 2.2) * 0.9;
    const ripple =
      Math.sin(u * Math.PI * rollFreq + rollPhase + v * 0.55) *
      0.26 *
      (0.45 + 0.55 * Math.sin(v * Math.PI));
    const dx = u - (foldX + (v - 0.5) * 0.03);
    const dy = v - (foldY + (u - 0.5) * 0.02);
    const vFold = -Math.exp(-(dx * dx) / (2 * 0.032 * 0.032)) * 0.36;
    const hFold = -Math.exp(-(dy * dy) / (2 * 0.036 * 0.036)) * 0.28;
    const diagD = (u - 0.72 + diagShift) * 0.72 - (v - 0.24) * 0.69;
    const diagBuckle =
      Math.exp(-(diagD * diagD) / (2 * 0.042 * 0.042)) *
      0.28 *
      Math.max(0, 1 - Math.hypot(u - 0.8, v - 0.22) * 1.45);
    return edgeCurlL + edgeCurlR + ripple + vFold + hFold + diagBuckle;
  };

  const eps = 1 / sw;
  for (let py = 0; py < sh; py++) {
    const v = py / (sh - 1);
    for (let px = 0; px < sw; px++) {
      const u = px / (sw - 1);
      const dzdx = (heightAt(u + eps, v) - heightAt(u - eps, v)) / (2 * eps);
      const dzdy = (heightAt(u, v + eps) - heightAt(u, v - eps)) / (2 * eps);
      const shade = (-dzdx * 0.74 - dzdy * 0.66) * 0.022;
      const idx = (py * sw + px) * 4;
      if (shade >= 0) {
        d[idx] = 248;
        d[idx + 1] = 244;
        d[idx + 2] = 236;
        d[idx + 3] = Math.min(48, Math.round(shade * 220));
      } else {
        d[idx] = 10;
        d[idx + 1] = 12;
        d[idx + 2] = 16;
        d[idx + 3] = Math.min(78, Math.round(-shade * 270));
      }
    }
  }
  sctx.putImageData(img, 0, 0);
  ctx.drawImage(small, 0, 0, w, h);
}

// ---------------------------------------------------------------------------
// 6) LGS Veteran Grime (bottom-left deck outline + warm wrist-rest patina)
// ---------------------------------------------------------------------------
function paintLgsGrime(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  seed: number,
  isDark: boolean,
): void {
  const rng = mulberry32(seed * 67 + 41);
  const n = makeNoise(seed * 19 + 3);
  const sw = 240;
  const sh = 150;
  const small = makeOffscreen(sw, sh);
  if (!small) return;
  const sctx = small.getContext("2d");
  if (!sctx) return;

  const img = sctx.createImageData(sw, sh);
  const d = img.data;
  const maxA = isDark ? 0.56 : 0.44;
  for (let py = 0; py < sh; py++) {
    const v = py / (sh - 1);
    for (let px = 0; px < sw; px++) {
      const u = px / (sw - 1);
      const leftWrist = Math.exp(
        -Math.pow((u - 0.26) / 0.24, 2) - Math.pow((v - 0.94) / 0.16, 2),
      );
      const rightWrist = Math.exp(
        -Math.pow((u - 0.74) / 0.24, 2) - Math.pow((v - 0.94) / 0.16, 2),
      );
      const wristBand = Math.min(
        1,
        leftWrist +
          rightWrist +
          Math.exp(-Math.pow((v - 0.97) / 0.1, 2)) * 0.4,
      );
      const corner =
        Math.max(0, Math.hypot((u - 0.5) * 1.62, (v - 0.5) * 1.62) - 0.78) *
        0.72;
      const blotch = n.fbm(u * 7.5, v * 5.2, 4);
      const grime = Math.min(
        maxA,
        Math.max(0, (wristBand * 0.46 + corner * 0.48) * (0.58 + blotch * 0.85)),
      );
      const idx = (py * sw + px) * 4;
      d[idx] = 74;
      d[idx + 1] = 60;
      d[idx + 2] = 44;
      d[idx + 3] = Math.round(grime * 255);
    }
  }
  sctx.putImageData(img, 0, 0);

  ctx.save();
  ctx.globalCompositeOperation = "multiply";
  ctx.drawImage(small, 0, 0, w, h);
  ctx.restore();

  ctx.save();
  ctx.globalAlpha = isDark ? 0.34 : 0.16;
  ctx.drawImage(small, 0, 0, w, h);
  ctx.restore();

  // Soft blurred deck-stack footprint in the BOTTOM LEFT corner (per user specification)
  ctx.save();
  if ("filter" in ctx) ctx.filter = "blur(6px)";
  const lx = w * 0.065;
  const ly = h * 0.555;
  const lw = w * 0.145;
  const lh = h * 0.345;
  ctx.strokeStyle = isDark
    ? "rgba(180, 166, 144, 0.2)"
    : "rgba(72, 58, 42, 0.26)";
  ctx.lineWidth = 7;
  ctx.strokeRect(lx, ly, lw, lh);
  ctx.restore();

  ctx.save();
  ctx.globalCompositeOperation = "multiply";
  for (let i = 0; i < 30; i++) {
    const x = 24 + rng() * (w - 48);
    const y = 24 + rng() * (h - 48);
    const rx = 6 + rng() * 18;
    const ry = 2.5 + rng() * 6;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(rng() * Math.PI);
    const g = ctx.createRadialGradient(0, 0, 1, 0, 0, rx);
    g.addColorStop(
      0,
      `rgba(58, 46, 32, ${(0.1 + rng() * 0.12).toFixed(2)})`,
    );
    g.addColorStop(1, "rgba(58, 46, 32, 0)");
    ctx.fillStyle = g;
    ctx.scale(1, ry / rx);
    ctx.beginPath();
    ctx.arc(0, 0, rx, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }
  ctx.restore();
}

// ---------------------------------------------------------------------------
// 7) Ember Scorched
// ---------------------------------------------------------------------------
function paintScorched(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  seed: number,
): void {
  const rng = mulberry32(seed * 113 + 7);
  const n = makeNoise(seed * 41 + 17);

  const cornerIdx = Math.floor(rng() * 4);
  const sx =
    cornerIdx % 2 === 0
      ? w * (0.03 + rng() * 0.06)
      : w * (0.91 + rng() * 0.06);
  const sy =
    cornerIdx < 2 ? h * (0.04 + rng() * 0.08) : h * (0.87 + rng() * 0.08);
  const maxR = 172 + rng() * 48;

  const sw = 240;
  const sh = 150;
  const small = makeOffscreen(sw, sh);
  if (!small) return;
  const sctx = small.getContext("2d");
  if (!sctx) return;

  const img = sctx.createImageData(sw, sh);
  const d = img.data;
  const scx = (sx / w) * sw;
  const scy = (sy / h) * sh;
  const sr = (maxR / w) * sw;

  for (let py = 0; py < sh; py++) {
    for (let px = 0; px < sw; px++) {
      const dist = Math.hypot((px - scx) * 0.92, (py - scy) * 1.04);
      const warp = (n.fbm(px * 0.085, py * 0.085, 4) - 0.5) * sr * 0.68;
      const t = 1 - (dist + warp) / sr;
      if (t <= 0) continue;
      const idx = (py * sw + px) * 4;
      if (t < 0.52) {
        const k = t / 0.52;
        d[idx] = Math.round(156 - k * 102);
        d[idx + 1] = Math.round(86 - k * 62);
        d[idx + 2] = Math.round(30 - k * 22);
        d[idx + 3] = Math.round(Math.pow(k, 1.15) * 0.72 * 255);
      } else {
        const k = Math.min(1, (t - 0.52) / 0.48);
        d[idx] = Math.round(44 - k * 34);
        d[idx + 1] = Math.round(22 - k * 16);
        d[idx + 2] = Math.round(11 - k * 7);
        d[idx + 3] = Math.round((0.72 + k * 0.25) * 255);
      }
    }
  }
  sctx.putImageData(img, 0, 0);

  ctx.save();
  ctx.drawImage(small, 0, 0, w, h);
  ctx.globalCompositeOperation = "multiply";
  ctx.globalAlpha = 0.55;
  ctx.drawImage(small, 0, 0, w, h);
  ctx.restore();

  for (let i = 0; i < 65; i++) {
    const ang = rng() * Math.PI * 2;
    const dist = rng() * maxR * 0.85;
    const ax = sx + Math.cos(ang) * dist;
    const ay = sy + Math.sin(ang) * dist;
    if (ax < 8 || ax > w - 8 || ay < 8 || ay > h - 8) continue;
    ctx.fillStyle = `rgba(182, 174, 162, ${(0.14 + rng() * 0.24).toFixed(2)})`;
    ctx.beginPath();
    ctx.arc(ax, ay, 0.7 + rng() * 1.3, 0, Math.PI * 2);
    ctx.fill();
  }

  for (let i = 0; i < 5; i++) {
    const ang = rng() * Math.PI * 2;
    const dist = maxR * (0.72 + rng() * 1.15);
    const bx = Math.max(24, Math.min(w - 24, sx + Math.cos(ang) * dist));
    const by = Math.max(24, Math.min(h - 24, sy + Math.sin(ang) * dist));
    const br = 3.2 + rng() * 5.2;
    const bg = ctx.createRadialGradient(bx, by, br * 0.12, bx, by, br * 2.2);
    bg.addColorStop(0, "rgba(10, 8, 7, 0.92)");
    bg.addColorStop(0.4, "rgba(48, 24, 10, 0.74)");
    bg.addColorStop(0.75, "rgba(146, 84, 30, 0.34)");
    bg.addColorStop(1, "rgba(146, 84, 30, 0)");
    ctx.fillStyle = bg;
    ctx.beginPath();
    ctx.arc(bx, by, br * 2.2, 0, Math.PI * 2);
    ctx.fill();
  }
}

/**
 * Paint a Playmat Effect overlay onto an existing playmat canvas of any size.
 * Bypasses cleanly with zero work when `effectId === "clean"`.
 */
export function paintPlaymatEffectOverlay(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  effectId: PlaymatEffectId,
  seed: number,
  isDarkSurface: boolean,
): void {
  if (effectId === "clean") return;
  const spec = playmatEffectSpec(effectId);
  const safeSeed = normalizePlaymatEffectSeed(seed, spec.defaultSeed);

  ctx.save();
  ctx.scale(width / 960, height / 600);
  switch (effectId) {
    case "dusty":
      paintDusty(ctx, 960, 600, safeSeed, isDarkSurface);
      break;
    case "oil_stained":
      paintOilStained(ctx, 960, 600, safeSeed, isDarkSurface);
      break;
    case "frayed":
      paintFrayed(ctx, 960, 600, safeSeed);
      break;
    case "creased":
      paintCreased(ctx, 960, 600, safeSeed);
      break;
    case "drink_spill":
      paintDrinkSpill(ctx, 960, 600, safeSeed, isDarkSurface);
      break;
    case "lgs_grime":
      paintLgsGrime(ctx, 960, 600, safeSeed, isDarkSurface);
      break;
    case "scorched":
      paintScorched(ctx, 960, 600, safeSeed);
      break;
  }
  ctx.restore();
}
