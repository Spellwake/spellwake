import type { PlayerDomain } from "@spellwake/application-contracts";
import { targetKey, type CombatTarget } from "./combat-draft";

/**
 * Assigning combat damage, as approved (DA-01 to DA-10): each source starts
 * from a suggested split, and a minus or plus beside each target moves one
 * point at a time. Only Confirm submits, and it sends every source together.
 * Pure, so every press can be tested.
 */

export type DamageDomain = Extract<PlayerDomain, { kind: "combat-damage" }>;
export type DamageSource = DamageDomain["sources"][number];

export interface DamageDraft {
  /** The source whose targets carry the controls. */
  active: string | null;
  /** Each source's points, in the order its targets were offered. */
  amounts: ReadonlyMap<string, readonly number[]>;
}

const count = (text: string): number => Number(text) || 0;
const sum = (values: readonly number[]): number =>
  values.reduce((total, value) => total + value, 0);
const sourceOf = (domain: DamageDomain, handle: string) =>
  domain.sources.find((source) => source.source === handle);

/** The least each target must hold before any damage may reach a player. */
function lethalFloors(source: DamageSource): number[] {
  return source.targets.map((target) =>
    target.kind === "object"
      ? count(
          source.trampleLethal?.find((row) => row.blocker === target.handle)
            ?.amount ?? "0",
        )
      : 0,
  );
}
const seatIndex = (source: DamageSource): number =>
  source.trampleLethal?.length
    ? source.targets.findIndex((target) => target.kind === "seat")
    : -1;

/** Damage reaches a player only past lethal damage to every blocker. */
function tramplesLegally(
  source: DamageSource,
  amounts: readonly number[],
): boolean {
  const seat = seatIndex(source);
  if (seat < 0 || (amounts[seat] ?? 0) === 0) return true;
  return lethalFloors(source).every(
    (floor, index) => (amounts[index] ?? 0) >= floor,
  );
}

/**
 * A legal starting split: lethal damage to each blocker in the order offered
 * and the rest to the player when the source tramples, otherwise everything
 * on the first target. The game stays the judge of what is legal.
 */
export function suggestDamage(source: DamageSource): number[] {
  const total = count(source.total);
  const amounts = source.targets.map(() => 0);
  if (amounts.length === 0) return amounts;
  const seat = seatIndex(source);
  if (seat < 0) {
    amounts[0] = total;
    return amounts;
  }
  let left = total;
  lethalFloors(source).forEach((floor, index) => {
    const given = Math.min(floor, left);
    amounts[index] = given;
    left -= given;
  });
  // Anything left only after every blocker holds lethal goes to the player.
  amounts[seat]! += left;
  return amounts;
}

/** A new decision: every source suggested, the first one active. */
export function startDamage(domain: DamageDomain): DamageDraft {
  return {
    active: domain.sources[0]?.source ?? null,
    amounts: new Map(
      domain.sources.map((source) => [source.source, suggestDamage(source)]),
    ),
  };
}

export const amountsOf = (
  draft: DamageDraft,
  source: DamageSource,
): readonly number[] =>
  draft.amounts.get(source.source) ?? source.targets.map(() => 0);
export const assignedOf = (draft: DamageDraft, source: DamageSource): number =>
  sum(amountsOf(draft, source));

/** Choose which source's targets carry the controls. Nothing else changes. */
export function selectSource(
  draft: DamageDraft,
  domain: DamageDomain,
  handle: string,
): DamageDraft {
  return sourceOf(domain, handle) && draft.active !== handle
    ? { ...draft, active: handle }
    : draft;
}

/**
 * The target a plus would take a point from once everything is assigned: the
 * last offered target that can spare one, never the receiver, and never a
 * blocker that would drop below lethal while damage reaches a player.
 */
function donor(
  source: DamageSource,
  amounts: readonly number[],
  receiver: number,
): number {
  const seat = seatIndex(source);
  for (let index = amounts.length - 1; index >= 0; index -= 1) {
    if (index === receiver || (amounts[index] ?? 0) === 0) continue;
    const next = amounts.map((value, at) =>
      at === index ? value - 1 : at === receiver ? value + 1 : value,
    );
    if (seat >= 0 && !tramplesLegally(source, next)) continue;
    return index;
  }
  return -1;
}

export const canMinus = (
  draft: DamageDraft,
  source: DamageSource,
  target: number,
): boolean => (amountsOf(draft, source)[target] ?? 0) > 0;

export function canPlus(
  draft: DamageDraft,
  source: DamageSource,
  target: number,
): boolean {
  const amounts = amountsOf(draft, source);
  if (target < 0 || target >= amounts.length) return false;
  const total = count(source.total);
  if (sum(amounts) < total) {
    // A point from the pool may go anywhere it leaves the split legal.
    const next = amounts.map((value, at) =>
      at === target ? value + 1 : value,
    );
    return tramplesLegally(source, next);
  }
  return donor(source, amounts, target) >= 0;
}

/** Press minus or plus beside one target of one source. */
export function adjustDamage(
  draft: DamageDraft,
  domain: DamageDomain,
  handle: string,
  target: number,
  delta: 1 | -1,
): DamageDraft {
  const source = sourceOf(domain, handle);
  if (!source) return draft;
  const amounts = [...amountsOf(draft, source)];
  if (delta < 0) {
    if (!canMinus(draft, source, target)) return draft;
    amounts[target]! -= 1;
  } else {
    if (!canPlus(draft, source, target)) return draft;
    if (sum(amounts) >= count(source.total)) {
      const from = donor(source, amounts, target);
      amounts[from]! -= 1;
    }
    amounts[target]! += 1;
  }
  const next = new Map(draft.amounts);
  next.set(handle, amounts);
  return { ...draft, amounts: next };
}

/** One source is complete: all of its damage assigned, legally. */
export function sourceComplete(
  draft: DamageDraft,
  source: DamageSource,
): boolean {
  const amounts = amountsOf(draft, source);
  return (
    sum(amounts) === count(source.total) && tramplesLegally(source, amounts)
  );
}
/** Confirm can be sent: every source is complete. */
export const canConfirmDamage = (
  draft: DamageDraft,
  domain: DamageDomain,
): boolean => domain.sources.every((source) => sourceComplete(draft, source));

/** What Confirm sends: one row for each target that receives damage. */
export function damageAssignments(
  draft: DamageDraft,
  domain: DamageDomain,
): { source: string; target: CombatTarget; amount: string }[] {
  return domain.sources.flatMap((source) =>
    amountsOf(draft, source).flatMap((amount, index) => {
      const target = source.targets[index];
      return target && amount > 0
        ? [{ source: source.source, target, amount: String(amount) }]
        : [];
    }),
  );
}

const shape = (source: DamageSource): string =>
  `${source.total}|${source.targets.map(targetKey).join(",")}|${(
    source.trampleLethal ?? []
  )
    .map((row) => `${row.blocker}:${row.amount}`)
    .join(",")}`;

/**
 * After the game changes under the same decision: a source offered exactly as
 * before keeps its split; any other starts again from its suggestion.
 */
export function reconcileDamage(
  draft: DamageDraft,
  previous: DamageDomain | null,
  domain: DamageDomain,
): { draft: DamageDraft; changed: boolean } {
  let changed = false;
  const amounts = new Map<string, readonly number[]>();
  for (const source of domain.sources) {
    const before = previous ? sourceOf(previous, source.source) : undefined;
    const kept = draft.amounts.get(source.source);
    if (kept && before && shape(before) === shape(source)) {
      amounts.set(source.source, kept);
    } else {
      amounts.set(source.source, suggestDamage(source));
      changed = true;
    }
  }
  if (amounts.size !== draft.amounts.size) changed = true;
  const active =
    draft.active && sourceOf(domain, draft.active)
      ? draft.active
      : (domain.sources[0]?.source ?? null);
  if (active !== draft.active) changed = true;
  return { draft: changed ? { active, amounts } : draft, changed };
}
