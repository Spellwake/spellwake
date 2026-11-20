/** Public choice descriptors and pure contextual checks, not source authorization. */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";
import {
  nativeRevisionSchema,
  playerBasisSchema,
  playerHandleSchema,
  rulesCountSchema,
  rulesIntegerSchema,
  type PlayerBasis,
  playerDisplayTextSchema,
} from "./player-primitives.ts";
import {
  PLAYER_ITEMS,
  playerManaSpendSchema,
  playerTargetSchema,
  playerTargetKey,
  uniquePlayerValues,
  type PlayerChoice,
  type PlayerCommand,
  type PlayerTarget,
} from "./player-actions.ts";
export const playerOptionSchema = z.strictObject({
  handle: playerHandleSchema,
  label: playerDisplayTextSchema(1024).refine((v) => v.length > 0),
});
const handles = z
  .array(playerHandleSchema)
  .max(PLAYER_ITEMS)
  .refine((xs) => uniquePlayerValues(xs, (x) => x));
const targets = z
  .array(playerTargetSchema)
  .max(PLAYER_ITEMS)
  .refine((xs) => uniquePlayerValues(xs, playerTargetKey));
const options = z
  .array(playerOptionSchema)
  .max(PLAYER_ITEMS)
  .refine((xs) => uniquePlayerValues(xs, (x) => x.handle));
const spend = z
  .array(playerManaSpendSchema)
  .max(6)
  .refine((xs) => uniquePlayerValues(xs, (x) => x.color));
const legend = z.strictObject({
  conflict: playerHandleSchema,
  objects: handles.refine((xs) => xs.length > 0),
});
const attacker = z.strictObject({
  attacker: playerHandleSchema,
  defenders: targets.refine(
    (xs) => xs.length > 0 && xs.every((t) => t.kind !== "stack"),
  ),
});
const blocker = z
  .strictObject({
    blocker: playerHandleSchema,
    attackers: handles,
    minimum: rulesCountSchema,
    maximum: rulesCountSchema,
  })
  .refine(
    (v) =>
      BigInt(v.minimum) <= BigInt(v.maximum) &&
      BigInt(v.maximum) <= BigInt(v.attackers.length),
  );
const damage = z
  .strictObject({
    source: playerHandleSchema,
    targets,
    total: rulesCountSchema,
  })
  .refine((v) => BigInt(v.total) === 0n || v.targets.length > 0);
const prevention = z.strictObject({
  attempt: playerHandleSchema,
  maximum: rulesCountSchema,
  label: playerDisplayTextSchema(1024).refine((value) => value.length > 0),
});
const rawDomain = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("options"),
    options,
    query: playerHandleSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("objects"),
    objects: handles,
    minimum: rulesCountSchema,
    maximum: rulesCountSchema,
  }),
  z.strictObject({
    kind: z.literal("ordering"),
    objects: handles,
    // Non-object references, such as token previews and trigger instances,
    // need source descriptions without becoming phantom physical objects.
    labels: options,
  }),
  z.strictObject({
    kind: z.literal("number"),
    minimum: rulesIntegerSchema,
    maximum: rulesIntegerSchema,
  }),
  z.strictObject({
    kind: z.literal("targets"),
    targets: targets.refine((xs) => xs.length > 0),
  }),
  z.strictObject({
    kind: z.literal("legend"),
    conflicts: z
      .array(legend)
      .min(1)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.conflict)),
  }),
  z.strictObject({
    kind: z.literal("payment"),
    minimumSpend: spend,
    maximumSpend: spend,
    total: rulesCountSchema,
    allowDecline: z.boolean(),
  }),
  z.strictObject({
    kind: z.literal("distribution"),
    targets,
    required: rulesCountSchema,
    minimumPerTarget: rulesCountSchema,
  }),
  z.strictObject({
    kind: z.literal("attackers"),
    choices: z
      .array(attacker)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.attacker)),
    required: handles,
  }),
  z.strictObject({
    kind: z.literal("blockers"),
    choices: z
      .array(blocker)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.blocker)),
    required: handles,
  }),
  z.strictObject({
    kind: z.literal("combat-damage"),
    sources: z
      .array(damage)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.source)),
  }),
  z.strictObject({
    kind: z.literal("prevention"),
    attempts: z
      .array(prevention)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.attempt)),
    required: rulesCountSchema,
  }),
]);
type RawDomain = z.infer<typeof rawDomain>;
const total = (amounts: readonly string[]): bigint =>
  amounts.reduce((sum, x) => sum + BigInt(x), 0n);
const manaAmount = (
  xs: readonly z.infer<typeof playerManaSpendSchema>[],
  color: string,
): bigint => BigInt(xs.find((x) => x.color === color)?.amount ?? "0");
function validDomain(v: RawDomain): boolean {
  switch (v.kind) {
    case "options":
      return v.options.length > 0 || v.query !== null;
    case "objects":
      return (
        BigInt(v.minimum) <= BigInt(v.maximum) &&
        BigInt(v.maximum) <= BigInt(v.objects.length)
      );
    case "ordering":
      return (
        v.labels.length === v.objects.length &&
        v.labels.every((entry) => v.objects.includes(entry.handle))
      );
    case "number":
      return BigInt(v.minimum) <= BigInt(v.maximum);
    case "payment":
      return (
        v.minimumSpend.every(
          (x) => BigInt(x.amount) <= manaAmount(v.maximumSpend, x.color),
        ) &&
        total(v.minimumSpend.map((x) => x.amount)) <= BigInt(v.total) &&
        total(v.maximumSpend.map((x) => x.amount)) >= BigInt(v.total)
      );
    case "distribution":
      return (
        (v.targets.length > 0 || BigInt(v.required) === 0n) &&
        (BigInt(v.required) === 0n ||
          BigInt(v.minimumPerTarget) <= BigInt(v.required))
      );
    case "attackers":
      return v.required.every((x) => v.choices.some((c) => c.attacker === x));
    case "blockers":
      return v.required.every((x) => v.choices.some((c) => c.blocker === x));
    case "prevention":
      return total(v.attempts.map((x) => x.maximum)) >= BigInt(v.required);
    default:
      return true;
  }
}
export const playerDomainSchema = rawDomain.refine(validDomain);
export const playerDecisionKindSchema = z.enum([
  "scry", "surveil", "order-library-graveyard", "zone-replacement",
  "replacement-chooser",
  "select-resolution",
  "order-resolution-graveyard",
  "optional-effect",
  "resolution-payment",
  "timestamp-order",
  "discard",
  "order-graveyard",
  "keep-legend",
  "order-triggers",
  "trigger-target",
  "declare-attackers",
  "declare-blockers",
  "assign-combat-damage",
  "mulligan",
  "bottom",
  "copy-targets",
  "damage-order",
  "allocate-prevention",
  "mana-color",
  "creature-type",
  "modes",
  "card-name",
  "number",
  "distribution",
]);
export const playerDeadlineSchema = z.string().refine((v) => {
  if (
    v.length !== 27 ||
    v.startsWith("0000") ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{6}Z$/.test(v)
  )
    return false;
  const date = new Date(v);
  return (
    Number.isFinite(date.getTime()) &&
    date.toISOString().slice(0, 19) === v.slice(0, 19)
  );
});
export const playerDecisionSchema = z.strictObject({
  decision: playerHandleSchema,
  basis: playerBasisSchema,
  respondingSeatIds: z
    .array(identifierSchema)
    .min(1)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  visibility: z.enum(["public", "responding-seat", "sealed-until-complete"]),
  kind: playerDecisionKindSchema,
  domain: playerDomainSchema,
  deadlineAt: playerDeadlineSchema.nullable(),
});
export type PlayerDecision = z.infer<typeof playerDecisionSchema>;
export type PlayerDomain = z.infer<typeof playerDomainSchema>;
export const playerChoiceQuerySchema = z.strictObject({
  basis: playerBasisSchema,
  decision: playerHandleSchema,
  query: playerHandleSchema,
  cursor: playerHandleSchema.nullable(),
  limit: nativeRevisionSchema.refine(
    (v) => BigInt(v) >= 1n && BigInt(v) <= 100n,
  ),
});
export const playerChoicePageSchema = z
  .strictObject({
    basis: playerBasisSchema,
    viewerSeatId: identifierSchema,
    decision: playerHandleSchema,
    query: playerHandleSchema,
    cursor: playerHandleSchema.nullable(),
    options: options.refine((xs) => xs.length <= 100),
    nextCursor: playerHandleSchema.nullable(),
  })
  .refine(
    (v) =>
      v.nextCursor === null ||
      (v.options.length > 0 && v.nextCursor !== v.cursor),
  );
export type PlayerChoiceQuery = z.infer<typeof playerChoiceQuerySchema>;
export type PlayerChoicePage = z.infer<typeof playerChoicePageSchema>;
export function samePlayerBasis(a: PlayerBasis, b: PlayerBasis): boolean {
  return (
    a.gameId === b.gameId &&
    a.contextId === b.contextId &&
    a.epoch === b.epoch &&
    a.revision === b.revision
  );
}
function supports(domain: PlayerDomain, kind: PlayerDecision["kind"]): boolean {
  switch (domain.kind) {
    case "options":
      return (kind === "zone-replacement" && domain.query === null) || [
        "replacement-chooser",
        "optional-effect",
        "mulligan",
        "copy-targets",
        "damage-order",
        "mana-color",
        "creature-type",
        "modes",
        "card-name",
      ].includes(kind);
    case "objects":
      return (
        kind === "discard" ||
        (kind === "bottom" && domain.minimum === domain.maximum) ||
        (kind === "select-resolution" &&
          domain.minimum === domain.maximum &&
          BigInt(domain.minimum) > 0n)
      );
    case "ordering":
      return (
        ["timestamp-order", "order-graveyard", "order-triggers"].includes(
          kind,
        ) ||
        (["order-resolution-graveyard","order-library-graveyard"].includes(kind) && domain.objects.length > 1) ||
        (["scry","surveil"].includes(kind) && domain.objects.length > 0)
      );
    case "number":
      return kind === "number";
    case "targets":
      return kind === "trigger-target";
    case "legend":
      return kind === "keep-legend";
    case "payment":
      return kind === "resolution-payment";
    case "distribution":
      return kind === "distribution";
    case "attackers":
      return kind === "declare-attackers";
    case "blockers":
      return kind === "declare-blockers";
    case "combat-damage":
      return kind === "assign-combat-damage";
    case "prevention":
      return kind === "allocate-prevention";
  }
}
/** Check parsed offered facts only; a boolean cannot establish their native provenance. */
export function validPlayerDecision(
  v: PlayerDecision,
  basis: PlayerBasis,
  viewer: string,
  roster: readonly string[],
  authorizedFullDisclosure = false,
): boolean {
  return (
    roster.length >= 1 &&
    roster.length <= 6 &&
    uniquePlayerValues(roster, (x) => x) &&
    roster.every((x) => identifierSchema.safeParse(x).success) &&
    samePlayerBasis(v.basis, basis) &&
    roster.includes(viewer) &&
    v.respondingSeatIds.every((x) => roster.includes(x)) &&
    (!["select-resolution", "order-resolution-graveyard"].includes(v.kind) ||
      v.respondingSeatIds.length === 1) &&
    (!["scry","surveil","order-library-graveyard","zone-replacement"].includes(v.kind) ||
      (v.respondingSeatIds.length === 1 && v.visibility === "responding-seat")) &&
    supports(v.domain, v.kind) &&
    (authorizedFullDisclosure ||
      v.visibility === "public" ||
      v.respondingSeatIds.includes(viewer))
  );
}
export function validPlayerChoicePage(
  v: PlayerChoicePage,
  q: PlayerChoiceQuery,
  viewer: string,
): boolean {
  return (
    samePlayerBasis(v.basis, q.basis) &&
    v.viewerSeatId === viewer &&
    v.decision === q.decision &&
    v.query === q.query &&
    v.cursor === q.cursor &&
    v.options.length <= Number(q.limit) &&
    (v.nextCursor === null || v.options.length === Number(q.limit))
  );
}
const hasTarget = (xs: readonly PlayerTarget[], x: PlayerTarget): boolean =>
  xs.some((t) => playerTargetKey(t) === playerTargetKey(x));
function allows(
  d: PlayerDomain,
  r: PlayerChoice,
  extra: readonly string[],
): boolean {
  switch (d.kind) {
    case "options":
      return (
        r.kind === "option" &&
        (d.options.some((x) => x.handle === r.option) ||
          (d.query !== null && extra.includes(r.option)))
      );
    case "objects":
      return (
        r.kind === "objects" &&
        BigInt(r.objects.length) >= BigInt(d.minimum) &&
        BigInt(r.objects.length) <= BigInt(d.maximum) &&
        r.objects.every((x) => d.objects.includes(x))
      );
    case "ordering":
      if (r.kind === "library-partition") return r.top.length + r.other.length === d.objects.length && [...r.top,...r.other].every(x=>d.objects.includes(x));
      return (
        r.kind === "ordering" &&
        r.ordered.length === d.objects.length &&
        r.ordered.every((x) => d.objects.includes(x))
      );
    case "number":
      return (
        r.kind === "number" &&
        BigInt(r.value) >= BigInt(d.minimum) &&
        BigInt(r.value) <= BigInt(d.maximum)
      );
    case "targets":
      return r.kind === "target" && hasTarget(d.targets, r.target);
    case "legend":
      return (
        r.kind === "legend" &&
        d.conflicts.some(
          (x) => x.conflict === r.conflict && x.objects.includes(r.keep),
        )
      );
    case "payment":
      return r.kind === "decline"
        ? d.allowDecline
        : r.kind === "pay" &&
            total(r.payment.spend.map((x) => x.amount)) === BigInt(d.total) &&
            d.minimumSpend.every(
              (x) => manaAmount(r.payment.spend, x.color) >= BigInt(x.amount),
            ) &&
            r.payment.spend.every(
              (x) => BigInt(x.amount) <= manaAmount(d.maximumSpend, x.color),
            );
    case "distribution":
      return (
        r.kind === "distribution" &&
        total(r.allocations.map((x) => x.amount)) === BigInt(d.required) &&
        r.allocations.every(
          (x) =>
            hasTarget(d.targets, x.target) &&
            (BigInt(x.amount) === 0n ||
              BigInt(x.amount) >= BigInt(d.minimumPerTarget)),
        )
      );
    case "attackers":
      return (
        r.kind === "attackers" &&
        d.required.every((x) => r.assignments.some((a) => a.attacker === x)) &&
        r.assignments.every((a) =>
          d.choices.some(
            (c) =>
              c.attacker === a.attacker && hasTarget(c.defenders, a.defender),
          ),
        )
      );
    case "blockers":
      return (
        r.kind === "blockers" &&
        d.required.every((x) => r.assignments.some((a) => a.blocker === x)) &&
        r.assignments.every((a) =>
          d.choices.some(
            (c) =>
              c.blocker === a.blocker &&
              BigInt(a.attackers.length) >= BigInt(c.minimum) &&
              BigInt(a.attackers.length) <= BigInt(c.maximum) &&
              a.attackers.every((x) => c.attackers.includes(x)),
          ),
        )
      );
    case "combat-damage":
      return (
        r.kind === "combat-damage" &&
        r.assignments.every((a) =>
          d.sources.some(
            (s) => s.source === a.source && hasTarget(s.targets, a.target),
          ),
        ) &&
        d.sources.every(
          (s) =>
            total(
              r.assignments
                .filter((a) => a.source === s.source)
                .map((x) => x.amount),
            ) === BigInt(s.total),
        )
      );
    case "prevention":
      return (
        r.kind === "prevention" &&
        total(r.allocations.map((x) => x.amount)) === BigInt(d.required) &&
        r.allocations.every((a) =>
          d.attempts.some(
            (s) =>
              s.attempt === a.attempt && BigInt(a.amount) <= BigInt(s.maximum),
          ),
        )
      );
  }
}
/** Validate only independently supplied frozen descriptor and page facts. */
export function validPlayerChoice(
  v: PlayerDecision,
  command: PlayerCommand,
  viewer: string,
  roster: readonly string[],
  pages: readonly PlayerChoicePage[] = [],
): boolean {
  if (
    command.action.kind !== "choose" ||
    !validPlayerDecision(v, command.basis, viewer, roster) ||
    !v.respondingSeatIds.includes(viewer) ||
    v.decision !== command.action.decision ||
    pages.length > 100
  )
    return false;
  const query = v.domain.kind === "options" ? v.domain.query : null;
  if (
    !pages.every(
      (p) =>
        samePlayerBasis(p.basis, v.basis) &&
        p.viewerSeatId === viewer &&
        p.decision === v.decision &&
        p.query === query,
    )
  )
    return false;
  if (["scry","surveil"].includes(v.kind) !== (command.action.response.kind === "library-partition")) return false;
  return allows(
    v.domain,
    command.action.response,
    pages.flatMap((p) => p.options.map((x) => x.handle)),
  );
}

/** Check typed target/visible-object references without interpreting native rules. */
export function playerDecisionReferences(
  d: PlayerDecision,
  objects: ReadonlySet<string>,
  stack: ReadonlySet<string>,
  roster: readonly string[],
): boolean {
  const target = (t: PlayerTarget): boolean =>
    t.kind === "seat"
      ? roster.includes(t.seatId)
      : t.kind === "stack"
        ? stack.has(t.handle)
        : objects.has(t.handle);
  switch (d.domain.kind) {
    case "targets":
    case "distribution":
      return d.domain.targets.every(target);
    case "objects":
      return d.domain.objects.every((x) => objects.has(x));
    case "ordering":
      return (
        !["order-graveyard", "order-resolution-graveyard"].includes(d.kind) ||
        d.domain.objects.every((x) => objects.has(x))
      );
    case "legend":
      return d.domain.conflicts.every((x) =>
        x.objects.every((o) => objects.has(o)),
      );
    case "attackers":
      return d.domain.choices.every(
        (x) => objects.has(x.attacker) && x.defenders.every(target),
      );
    case "blockers":
      return d.domain.choices.every(
        (x) =>
          objects.has(x.blocker) && x.attackers.every((o) => objects.has(o)),
      );
    case "combat-damage":
      return d.domain.sources.every(
        (x) => objects.has(x.source) && x.targets.every(target),
      );
    default:
      return true;
  }
}
