import type { PlayerDomain } from "@spellwake/application-contracts";

/**
 * Declaring attackers and blockers, as approved (AT-01 to AT-11, BL-01 to
 * BL-09): a local draft built by clicking, which only Attack, Block, No
 * Attacks or No Blocks submits. Pure, so every gesture can be tested.
 *
 * Attack: click an offered creature to select it, then an offered player or
 * card to assign it. Clicking an assigned creature removes its assignment.
 * Block: click an offered creature of yours, then an attacking creature. A
 * blocker that may block several keeps its selection while links toggle.
 */

export type AttackDomain = Extract<PlayerDomain, { kind: "attackers" }>;
export type BlockDomain = Extract<PlayerDomain, { kind: "blockers" }>;
export type CombatTarget = AttackDomain["choices"][number]["defenders"][number];

/** One key per target, so a seat and a card are never confused. */
export const targetKey = (target: CombatTarget): string =>
  target.kind === "seat"
    ? `seat:${target.seatId}`
    : target.kind === "object"
      ? `object:${target.handle}`
      : "stack";

export interface AttackDraft {
  /** The creature chosen and waiting for its defender. */
  pending: string | null;
  /** Each attacking creature's one defender. */
  assigned: ReadonlyMap<string, CombatTarget>;
}
export const EMPTY_ATTACK: AttackDraft = { pending: null, assigned: new Map() };

const offeredAttacker = (domain: AttackDomain, handle: string) =>
  domain.choices.find((choice) => choice.attacker === handle);

/** Click one of your creatures while declaring attackers. */
export function attackWithCreature(
  draft: AttackDraft,
  domain: AttackDomain,
  handle: string,
): AttackDraft {
  if (!offeredAttacker(domain, handle)) return draft;
  if (draft.assigned.has(handle)) {
    const assigned = new Map(draft.assigned);
    assigned.delete(handle);
    return { ...draft, assigned };
  }
  if (draft.pending === handle) return { ...draft, pending: null };
  // Only one creature attacks when the source says so: choosing another replaces it.
  if (domain.singleAttacker && draft.assigned.size > 0)
    return { pending: handle, assigned: new Map() };
  return { ...draft, pending: handle };
}

/** The defenders the creature waiting for one may be sent at. */
export function attackTargets(
  draft: AttackDraft,
  domain: AttackDomain,
): readonly CombatTarget[] {
  return draft.pending
    ? (offeredAttacker(domain, draft.pending)?.defenders ?? [])
    : [];
}

/** Click a player or a card as the waiting creature's defender. */
export function attackTarget(
  draft: AttackDraft,
  domain: AttackDomain,
  target: CombatTarget,
): AttackDraft {
  if (!draft.pending) return draft;
  const offered = attackTargets(draft, domain).some(
    (entry) => targetKey(entry) === targetKey(target),
  );
  if (!offered) return draft;
  const assigned = new Map(draft.assigned);
  assigned.set(draft.pending, target);
  return { pending: null, assigned };
}

/** Attack can be sent: something attacks, nothing waits, and every required creature attacks. */
export const canAttack = (draft: AttackDraft, domain: AttackDomain): boolean =>
  draft.assigned.size > 0 &&
  draft.pending === null &&
  domain.required.every((handle) => draft.assigned.has(handle));
/** Declaring no attackers is offered only when no creature must attack. */
export const canSkipAttack = (domain: AttackDomain): boolean =>
  domain.required.length === 0;

/** After the game changes: keep what is still offered, and never pick a new defender. */
export function reconcileAttack(
  draft: AttackDraft,
  domain: AttackDomain,
): { draft: AttackDraft; changed: boolean } {
  const assigned = new Map<string, CombatTarget>();
  for (const [handle, target] of draft.assigned) {
    const choice = offeredAttacker(domain, handle);
    if (
      choice?.defenders.some((entry) => targetKey(entry) === targetKey(target))
    )
      assigned.set(handle, target);
  }
  const pending =
    draft.pending && offeredAttacker(domain, draft.pending)
      ? draft.pending
      : null;
  const changed =
    assigned.size !== draft.assigned.size || pending !== draft.pending;
  return { draft: changed ? { pending, assigned } : draft, changed };
}

export interface BlockDraft {
  pending: string | null;
  /** Each blocker's attackers, in the order chosen. */
  assigned: ReadonlyMap<string, readonly string[]>;
}
export const EMPTY_BLOCK: BlockDraft = { pending: null, assigned: new Map() };

const offeredBlocker = (domain: BlockDomain, handle: string) =>
  domain.choices.find((choice) => choice.blocker === handle);
const limit = (text: string): number => Number(text) || 0;

/** Click one of your creatures while declaring blockers. */
export function blockWithCreature(
  draft: BlockDraft,
  domain: BlockDomain,
  handle: string,
): BlockDraft {
  if (!offeredBlocker(domain, handle)) return draft;
  // A blocker being edited is left alone by its own click only once it has no links.
  if (draft.pending === handle && !draft.assigned.has(handle))
    return { ...draft, pending: null };
  if (draft.assigned.has(handle)) {
    const assigned = new Map(draft.assigned);
    assigned.delete(handle);
    return {
      pending: draft.pending === handle ? null : draft.pending,
      assigned,
    };
  }
  if (domain.singleBlocker && draft.assigned.size > 0)
    return { pending: handle, assigned: new Map() };
  return { ...draft, pending: handle };
}

/** The attacking creatures the selected blocker may block. */
export function blockTargets(
  draft: BlockDraft,
  domain: BlockDomain,
): readonly string[] {
  return draft.pending
    ? (offeredBlocker(domain, draft.pending)?.attackers ?? [])
    : [];
}

/** Click an attacking creature for the selected blocker. */
export function blockAttacker(
  draft: BlockDraft,
  domain: BlockDomain,
  attacker: string,
): BlockDraft {
  const choice = draft.pending ? offeredBlocker(domain, draft.pending) : null;
  if (!draft.pending || !choice || !choice.attackers.includes(attacker))
    return draft;
  const current = draft.assigned.get(draft.pending) ?? [];
  const assigned = new Map(draft.assigned);
  const many = limit(choice.maximum) > 1;
  if (current.includes(attacker)) {
    // A blocker of several toggles one link; the rest stay.
    const rest = current.filter((entry) => entry !== attacker);
    if (rest.length > 0) assigned.set(draft.pending, rest);
    else assigned.delete(draft.pending);
    return { pending: draft.pending, assigned };
  }
  if (current.length >= limit(choice.maximum)) return draft;
  assigned.set(draft.pending, [...current, attacker]);
  // An ordinary blocker is finished by its one link.
  return { pending: many ? draft.pending : null, assigned };
}

/** Escape leaves selection, keeping every link already made. */
export const cancelPending = <Draft extends { pending: string | null }>(
  draft: Draft,
): Draft => (draft.pending === null ? draft : { ...draft, pending: null });

/** Block can be sent: the whole set meets every count the source set. */
export function canBlock(draft: BlockDraft, domain: BlockDomain): boolean {
  if (draft.assigned.size === 0) return false;
  // A selected blocker with no link yet is still waiting for its first.
  if (draft.pending && !draft.assigned.has(draft.pending)) return false;
  for (const [handle, attackers] of draft.assigned) {
    const choice = offeredBlocker(domain, handle);
    if (!choice) return false;
    if (attackers.length < Math.max(1, limit(choice.minimum))) return false;
    if (attackers.length > limit(choice.maximum)) return false;
  }
  if (!domain.required.every((handle) => draft.assigned.has(handle)))
    return false;
  // An attacker that needs several blockers is blocked by enough, or by none.
  for (const rule of domain.minimumIfBlocked ?? []) {
    const blockers = [...draft.assigned.values()].filter((attackers) =>
      attackers.includes(rule.attacker),
    ).length;
    if (blockers > 0 && blockers < limit(rule.minimum)) return false;
  }
  return true;
}
export const canSkipBlock = (domain: BlockDomain): boolean =>
  domain.required.length === 0;

export function reconcileBlock(
  draft: BlockDraft,
  domain: BlockDomain,
): { draft: BlockDraft; changed: boolean } {
  let changed = false;
  const assigned = new Map<string, readonly string[]>();
  for (const [handle, attackers] of draft.assigned) {
    const choice = offeredBlocker(domain, handle);
    const kept = choice
      ? attackers.filter((attacker) => choice.attackers.includes(attacker))
      : [];
    if (kept.length !== attackers.length) changed = true;
    if (kept.length > 0) assigned.set(handle, kept);
  }
  const pending =
    draft.pending && offeredBlocker(domain, draft.pending)
      ? draft.pending
      : null;
  if (pending !== draft.pending) changed = true;
  return { draft: changed ? { pending, assigned } : draft, changed };
}

/** The attacks already declared against the viewer, read from what may be blocked. */
export const publishedAttackers = (domain: BlockDomain): string[] => [
  ...new Set(domain.choices.flatMap((choice) => choice.attackers)),
];
