/** Bounded fan geometry and fold navigation shared by hands and pile inspection. */
import * as THREE from "three";

export type FanPose = { x: number; y: number; angle: number };
/** A bounded consecutive view around focus; source order and identities remain untouched. */
export function fanWindow(
  count: number,
  focus: number,
  limit = 9,
): { start: number; count: number } {
  const total = Math.max(0, Math.floor(count));
  const visible = Math.min(total, Math.max(1, Math.floor(limit)));
  return {
    start: THREE.MathUtils.clamp(
      Math.round(focus) - Math.floor(visible / 2),
      0,
      total - visible,
    ),
    count: visible,
  };
}
/** Compress only thickness in large fans so physical overlap stays ordered and bounded. */
export function fanThickness(count: number): number {
  return Math.min(1, 12 / Math.max(1, count));
}
/** Every sleeve has room in depth; only the selected card comes ahead of the fan. */
export function fanDepth(
  index: number,
  count: number,
  scale: number,
  selected: boolean,
): number {
  return (
    (selected ? count : index) * 0.06 * scale * fanThickness(count) +
    (selected ? 0.025 : 0)
  );
}
/** Dense piles overlap closely; spacing never stretches with the selection. */
export function compactFanSpan(count: number, span: number): number {
  return count > 12 ? Math.min(span, 1.4) : span;
}
/** Equal card spacing and rotation, without focus-dependent distortion. */
export function compactFan(count: number, span: number): FanPose[] {
  const arc = count > 12 ? 0.16 : 0.55;
  const width = compactFanSpan(count, span);
  return arcPoses(
    Array.from({ length: count }, (_, i) =>
      count > 1 ? (i / (count - 1)) * width : 0,
    ),
    width,
    arc,
  );
}
function arcPoses(
  offsets: readonly number[],
  width: number,
  arc: number,
): FanPose[] {
  const radius = width / (2 * Math.sin(arc / 2));
  return offsets.map((offset) => {
    const angle = width > 0 ? (offset / width - 0.5) * arc : 0;
    return {
      x: radius * Math.sin(angle),
      y: radius * (Math.cos(angle) - 1),
      angle: -angle,
    };
  });
}

/** One circle for hand and other-zone groups; group gaps change spacing, not curvature. */
export function groupedHandFan(
  count: number,
  zones: readonly string[],
  span: number,
  handOffsets?: readonly number[],
): { hand: FanPose[]; external: FanPose[]; width: number } {
  const handWidth = compactFanSpan(count, span);
  const offsets = handOffsets
    ? [...handOffsets]
    : Array.from({ length: count }, (_, i) =>
        count > 1 ? (i / (count - 1)) * handWidth : 0,
      );
  const handCount = offsets.length;
  if (!zones.length)
    return {
      hand: handOffsets
        ? arcPoses(offsets, handWidth, 0.55)
        : compactFan(count, span),
      external: [],
      width: compactFanSpan(count, span) + 2.5,
    };
  let cursor = count ? handWidth : 0;
  const groupCounts = new Map<string, number>();
  for (const zone of zones)
    groupCounts.set(zone, (groupCounts.get(zone) ?? 0) + 1);
  for (const [index, zone] of zones.entries()) {
    if (index === 0) cursor += count ? 2.65 : 0;
    else
      cursor +=
        zone === zones[index - 1]
          ? Math.min(0.8, 2.4 / Math.max(1, (groupCounts.get(zone) ?? 1) - 1))
          : 2.65;
    offsets.push(cursor);
  }
  const poses = arcPoses(offsets, cursor, 0.65);
  return {
    hand: poses.slice(0, handCount),
    external: poses.slice(handCount),
    width: cursor + 3,
  };
}
/** Stable visible-strip selection ignores the enlarged hover card. */
export function fanIndexAt(
  x: number,
  poses: readonly FanPose[],
  halfCard: number,
): number {
  let index = 0;
  for (let i = 1; i < poses.length; i++)
    if (x >= (poses[i]?.x ?? 0) - halfCard) index = i;
  return index;
}

/** Focus moves through a fixed fan; it never changes card order or creates pages. */
export function createFanFocus() {
  let position = 0,
    maximum = 0,
    velocity = 0,
    dragging = false;
  const clamp = (value: number): number =>
    THREE.MathUtils.clamp(value, 0, maximum);
  return {
    configure(count: number): void {
      maximum = Math.max(0, count - 1);
      position = clamp(position);
    },
    reset(value = 0): void {
      position = clamp(value);
      velocity = 0;
      dragging = false;
    },
    begin(): void {
      dragging = true;
      velocity = 0;
    },
    pan(cards: number, seconds: number): void {
      position = clamp(position + cards);
      velocity = THREE.MathUtils.clamp(
        cards / Math.max(seconds, 0.008),
        -80,
        80,
      );
    },
    end(paused: boolean): void {
      dragging = false;
      if (paused) velocity = 0;
    },
    move(cards: number): void {
      velocity = 0;
      position = clamp(position + cards);
    },
    tick(elapsed: number, reduced: boolean): boolean {
      if (dragging) return false;
      if (reduced || Math.abs(velocity) < 0.02) {
        velocity = 0;
        return false;
      }
      const decay = Math.exp(-elapsed * 8);
      const next = position + (velocity * (1 - decay)) / 8;
      position = clamp(next);
      velocity = next === position ? velocity * decay : 0;
      return Math.abs(velocity) >= 0.02;
    },
    get position(): number {
      return position;
    },
  };
}
