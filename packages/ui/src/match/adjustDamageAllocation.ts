/** Increase from the unassigned pool first. With a full split, transfer from the
 * first other recipient above its minimum, in the host's display order. */
export function adjustDamageAllocation(
  values: readonly [number, number],
  target: number,
  delta: 1 | -1,
  total: number,
  minimum: number,
): [number, number];
export function adjustDamageAllocation(
  values: readonly number[],
  target: number,
  delta: 1 | -1,
  total: number,
  minimum: number,
): number[];
export function adjustDamageAllocation(
  values: readonly number[],
  target: number,
  delta: 1 | -1,
  total: number,
  minimum: number,
): number[] {
  const next = [...values];
  const current = next[target];
  if (current === undefined || (delta < 0 && current <= minimum)) return next;
  if (delta > 0 && next.reduce((sum, n) => sum + n, 0) === total) {
    const donor = next.findIndex((n, i) => i !== target && n > minimum);
    if (donor < 0) return next;
    next[donor] = next[donor]! - 1;
  }
  next[target] = current + delta;
  return next;
}
