/**
 * Timing for the pack opening. The curves are the app's motion language
 * (arrive, depart, travel), passed in by the host so the 3D sequence keeps
 * the same pulse as the screens around it.
 */

/** A CSS-style cubic Bézier, as its two control points. */
export type Curve = readonly [number, number, number, number];
export type Ease = (t: number) => number;

/**
 * Evaluate a CSS cubic-bezier timing function. Newton steps with a bisection
 * fallback, so steep curves near the ends still converge.
 */
export function bezier([x1, y1, x2, y2]: Curve): Ease {
  const ax = 3 * x1 - 3 * x2 + 1;
  const bx = 3 * x2 - 6 * x1;
  const cx = 3 * x1;
  const ay = 3 * y1 - 3 * y2 + 1;
  const by = 3 * y2 - 6 * y1;
  const cy = 3 * y1;
  const sampleX = (s: number): number => ((ax * s + bx) * s + cx) * s;
  const sampleY = (s: number): number => ((ay * s + by) * s + cy) * s;
  const slopeX = (s: number): number => (3 * ax * s + 2 * bx) * s + cx;
  return (t: number): number => {
    if (t <= 0) return 0;
    if (t >= 1) return 1;
    let s = t;
    for (let step = 0; step < 6; step += 1) {
      const error = sampleX(s) - t;
      if (Math.abs(error) < 1e-6) return sampleY(s);
      const slope = slopeX(s);
      if (Math.abs(slope) < 1e-6) break;
      s -= error / slope;
    }
    let low = 0;
    let high = 1;
    s = t;
    for (let step = 0; step < 30; step += 1) {
      const x = sampleX(s);
      if (Math.abs(x - t) < 1e-6) break;
      if (x < t) low = s;
      else high = s;
      s = (low + high) / 2;
    }
    return sampleY(s);
  };
}

/** Progress of a window that starts at `start` and lasts `length`, clamped to 0–1. */
export function span(time: number, start: number, length: number): number {
  if (length <= 0) return time >= start ? 1 : 0;
  return Math.min(1, Math.max(0, (time - start) / length));
}

export const clamp01 = (value: number): number =>
  Math.min(1, Math.max(0, value));
export const mix = (from: number, to: number, amount: number): number =>
  from + (to - from) * amount;
export function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = clamp01((x - edge0) / (edge1 - edge0));
  return t * t * (3 - 2 * t);
}

/** The motion language's three curves. */
export type MotionCurves = { arrive: Curve; depart: Curve; travel: Curve };
/** Its durations, in seconds. */
export type MotionBeats = { scene: number; hero: number; stagger: number };

/**
 * When each part of the opening happens, in seconds from the moment the
 * cards are known and the tear begins. Derived from the host's beats so a
 * change to the motion language moves the whole sequence together.
 */
export type OpeningScript = ReturnType<typeof openingScript>;
export function openingScript(beats: MotionBeats, cardCount: number) {
  const { scene, hero, stagger } = beats;
  const anticipation = scene / 3;
  const tear = { start: anticipation * 0.7, length: hero };
  const tearEnd = tear.start + tear.length;
  const release = { start: tearEnd - scene * 0.2, length: hero * 0.9 };
  const mouth = { start: tear.start + hero * 0.55, length: scene * 1.4 };
  const rise = { start: tearEnd + scene * 0.35, length: hero };
  const exit = { start: rise.start + scene * 0.5, length: hero * 0.85 };
  const deal = {
    start: rise.start + rise.length * 0.95,
    each: hero * 0.78,
    stagger,
  };
  const dealEnd =
    deal.start + Math.max(0, cardCount - 1) * deal.stagger + deal.each;
  return {
    anticipation: { start: 0, length: anticipation },
    tear,
    release,
    mouth,
    burst: { start: tearEnd - scene * 0.1, length: scene * 1.6 },
    rise,
    exit,
    deal,
    end: dealEnd,
  };
}
