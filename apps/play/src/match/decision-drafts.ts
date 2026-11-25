import {
  playerTargetKey,
  type PlayerChoice,
  type PlayerDomain,
  type PlayerTarget,
} from "@spellwake/application-contracts";

/**
 * Answers a player builds up before sending: an order, a set of targets,
 * counters to remove, damage to prevent and a trigger's modes. Each draft
 * only ever holds what the game would accept, so the answer needs no second
 * check when it is sent.
 */
type Domain<Kind extends PlayerDomain["kind"]> = Extract<
  PlayerDomain,
  { kind: Kind }
>;
type Reply<Kind extends PlayerChoice["kind"]> = Extract<
  PlayerChoice,
  { kind: Kind }
>;
type CounterKind = Domain<"counter-removal">["available"][number]["kind"];

const has = (targets: readonly PlayerTarget[], target: PlayerTarget): boolean =>
  targets.some((other) => playerTargetKey(other) === playerTargetKey(target));

/** A list with one item moved to another place. Out-of-range moves change nothing. */
export function moved<Item>(
  items: readonly Item[],
  from: number,
  to: number,
): Item[] {
  const next = [...items];
  if (from < 0 || from >= next.length || to < 0 || to >= next.length)
    return next;
  const [item] = next.splice(from, 1);
  if (item !== undefined) next.splice(to, 0, item);
  return next;
}

/** A chosen set of targets with one added or taken away, never past the most allowed. */
export function toggledTarget(
  domain: Domain<"target-list">,
  chosen: readonly PlayerTarget[],
  target: PlayerTarget,
): PlayerTarget[] {
  if (!has(domain.targets, target)) return [...chosen];
  if (has(chosen, target))
    return chosen.filter(
      (other) => playerTargetKey(other) !== playerTargetKey(target),
    );
  return chosen.length < Number(domain.maximum)
    ? [...chosen, target]
    : [...chosen];
}
export const targetListReady = (
  domain: Domain<"target-list">,
  chosen: readonly PlayerTarget[],
): boolean =>
  chosen.length >= Number(domain.minimum) &&
  chosen.length <= Number(domain.maximum);

/** Counters to remove, by kind. */
export type CounterDraft = Readonly<Partial<Record<CounterKind, number>>>;
const counterTotal = (draft: CounterDraft): number =>
  Object.values(draft).reduce((sum, count) => sum + (count ?? 0), 0);
/** One more or one fewer of a kind, within what the permanent has and the most that may go. */
export function counterStep(
  domain: Domain<"counter-removal">,
  draft: CounterDraft,
  kind: CounterKind,
  delta: 1 | -1,
): CounterDraft {
  const available =
    domain.available.find((entry) => entry.kind === kind)?.count ?? 0;
  const current = draft[kind] ?? 0;
  const next = current + delta;
  if (next < 0 || next > available) return draft;
  if (delta > 0 && counterTotal(draft) >= domain.maximum) return draft;
  return { ...draft, [kind]: next };
}
export const counterLeft = (
  domain: Domain<"counter-removal">,
  draft: CounterDraft,
): number => domain.maximum - counterTotal(draft);
export function counterReply(draft: CounterDraft): Reply<"counter-removal"> {
  return {
    kind: "counter-removal",
    counters: (Object.entries(draft) as [CounterKind, number][])
      .filter(([, count]) => count > 0)
      .map(([kind, count]) => ({ kind, count })),
  };
}

/** Damage to prevent, by attempt. */
export type PreventionDraft = Readonly<Record<string, number>>;
const preventionTotal = (draft: PreventionDraft): number =>
  Object.values(draft).reduce((sum, amount) => sum + amount, 0);
/** How much of the required amount is still to be placed. */
export const preventionLeft = (
  domain: Domain<"prevention">,
  draft: PreventionDraft,
): number => Number(domain.required) - preventionTotal(draft);
/** One more or one fewer on an attempt, within its own most and the amount required. */
export function preventionStep(
  domain: Domain<"prevention">,
  draft: PreventionDraft,
  attempt: string,
  delta: 1 | -1,
): PreventionDraft {
  const most = Number(
    domain.attempts.find((entry) => entry.attempt === attempt)?.maximum ?? 0,
  );
  const next = (draft[attempt] ?? 0) + delta;
  if (next < 0 || next > most) return draft;
  if (delta > 0 && preventionLeft(domain, draft) <= 0) return draft;
  return { ...draft, [attempt]: next };
}
export function preventionReply(draft: PreventionDraft): Reply<"prevention"> {
  return {
    kind: "prevention",
    allocations: Object.entries(draft)
      .filter(([, amount]) => amount > 0)
      .map(([attempt, amount]) => ({ attempt, amount: String(amount) })),
  };
}

/** A trigger's chosen modes in the order chosen, each with its targets. */
export type ModesDraft = readonly {
  mode: number;
  targets: readonly PlayerTarget[];
}[];
type OfferedMode = Domain<"trigger-modes">["modes"][number];
/** How many targets a mode takes: none, exactly one, or a range. */
const modeRange = (mode: OfferedMode): [number, number] =>
  mode.multiple ?? (mode.targetRequired ? [1, 1] : [0, 0]);
const usedTargets = (draft: ModesDraft, except: number): PlayerTarget[] =>
  draft.flatMap((entry) => (entry.mode === except ? [] : entry.targets));

/** A mode added or taken away, never past the most the trigger allows. */
export function toggledMode(
  domain: Domain<"trigger-modes">,
  draft: ModesDraft,
  index: number,
): ModesDraft {
  if (draft.some((entry) => entry.mode === index))
    return draft.filter((entry) => entry.mode !== index);
  if (
    !domain.modes.some((mode) => mode.index === index) ||
    draft.length >= domain.maximum
  )
    return draft;
  return [...draft, { mode: index, targets: [] }];
}
/** Whether a target may still be given to a mode: offered, and not used by another where targets must differ. */
export function modeTargetOpen(
  domain: Domain<"trigger-modes">,
  draft: ModesDraft,
  index: number,
  target: PlayerTarget,
): boolean {
  const mode = domain.modes.find((offered) => offered.index === index);
  return (
    !!mode &&
    has(mode.targets, target) &&
    !(domain.distinctTargets && has(usedTargets(draft, index), target))
  );
}
/** A chosen mode's target set or cleared. A single-target mode swaps its target. */
export function toggledModeTarget(
  domain: Domain<"trigger-modes">,
  draft: ModesDraft,
  index: number,
  target: PlayerTarget,
): ModesDraft {
  const mode = domain.modes.find((offered) => offered.index === index);
  if (!mode || !modeTargetOpen(domain, draft, index, target)) return draft;
  const [, most] = modeRange(mode);
  return draft.map((entry) => {
    if (entry.mode !== index) return entry;
    if (has(entry.targets, target))
      return {
        ...entry,
        targets: entry.targets.filter(
          (other) => playerTargetKey(other) !== playerTargetKey(target),
        ),
      };
    if (most === 1) return { ...entry, targets: [target] };
    return entry.targets.length < most
      ? { ...entry, targets: [...entry.targets, target] }
      : entry;
  });
}
export function modesReady(
  domain: Domain<"trigger-modes">,
  draft: ModesDraft,
): boolean {
  return (
    draft.length >= domain.minimum &&
    draft.length <= domain.maximum &&
    draft.every((entry) => {
      const mode = domain.modes.find((offered) => offered.index === entry.mode);
      if (!mode) return false;
      const [least, most] = modeRange(mode);
      return entry.targets.length >= least && entry.targets.length <= most;
    })
  );
}
export function modesReply(
  domain: Domain<"trigger-modes">,
  draft: ModesDraft,
): Reply<"trigger-modes"> {
  return {
    kind: "trigger-modes",
    modes: draft.map((entry) => {
      const mode = domain.modes.find((offered) => offered.index === entry.mode);
      // A mode that takes a range names its targets as a list; any other names one or none.
      return mode?.multiple
        ? { mode: entry.mode, target: null, targets: [...entry.targets] }
        : { mode: entry.mode, target: entry.targets[0] ?? null };
    }),
  };
}

/**
 * What an attack cost takes when life pays for as many of its white
 * Phyrexian symbols as chosen: the rest is white mana, and the generic part
 * is paid from whatever the pool holds, white last.
 */
export function attackCostPayment(
  domain: Domain<"attack-cost">,
  pool: readonly { color: string; amount: number }[],
  lifeSymbols: number,
): Reply<"pay-attack-cost"> | null {
  const phyrexian = Number(domain.whitePhyrexian);
  if (lifeSymbols < 0 || lifeSymbols > phyrexian) return null;
  const left = new Map(pool.map((entry) => [entry.color, entry.amount]));
  const spend = new Map<string, number>();
  const take = (color: string, amount: number): number => {
    const taken = Math.min(amount, left.get(color) ?? 0);
    if (taken > 0) {
      left.set(color, (left.get(color) ?? 0) - taken);
      spend.set(color, (spend.get(color) ?? 0) + taken);
    }
    return taken;
  };
  const white = phyrexian - lifeSymbols;
  if (take("white", white) < white) return null;
  let generic = Number(domain.generic);
  for (const color of [...left.keys()].sort((a, b) =>
    a === "white" ? 1 : b === "white" ? -1 : 0,
  ))
    generic -= take(color, generic);
  if (generic > 0) return null;
  return {
    kind: "pay-attack-cost",
    lifeSymbols: String(lifeSymbols),
    payment: {
      spend: [...spend].map(([color, amount]) => ({
        color,
        amount: String(amount),
      })) as Reply<"pay-attack-cost">["payment"]["spend"],
    },
  };
}
