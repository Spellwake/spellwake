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
  playerCounterQuantitiesSchema,
  playerManaSpendSchema,
  playerManaColorSchema,
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
const bandQualities = ["legendary-creatures", "creatures-named-wolves-of-the-hunt"] as const;
const attacker = z.strictObject({
  bandQualities: z.array(z.enum(bandQualities)).max(bandQualities.length).optional(),
  bandsWithOther: z.array(z.enum(bandQualities)).max(bandQualities.length).optional(),
  banding: z.boolean().optional(),
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
    trampleLethal: z
      .array(
        z.strictObject({
          blocker: playerHandleSchema,
          amount: rulesCountSchema,
        }),
      )
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.blocker))
      .optional(),
  })
  .refine((v) => BigInt(v.total) === 0n || v.targets.length > 0)
  .refine(
    (v) =>
      !v.trampleLethal?.length ||
      (v.targets.filter((target) => target.kind === "seat").length === 1 &&
        v.targets.length === v.trampleLethal.length + 1 &&
        v.trampleLethal.every((row) =>
          v.targets.some(
            (target) =>
              target.kind === "object" && target.handle === row.blocker,
          ),
        )),
  );
const prevention = z.strictObject({
  attempt: playerHandleSchema,
  maximum: rulesCountSchema,
  label: playerDisplayTextSchema(1024).refine((value) => value.length > 0),
});
const rawDomain = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("trigger-modes"),
    minimum: z.number().int().min(0).max(255),
    maximum: z.number().int().min(0).max(255),
    distinctTargets: z.boolean(),
    modes: z.array(z.strictObject({
      index: z.number().int().min(0).max(255),
      targetRequired: z.boolean(),
      multiple: z.tuple([z.number().int().nonnegative(), z.number().int().nonnegative()]).optional(),
      targets,
    })).max(PLAYER_ITEMS),
  }),
  z.strictObject({kind:z.literal("madness-cast"),spell:playerHandleSchema,targetRequired:z.boolean(),targets,generic:rulesCountSchema,
    required:z.array(playerManaSpendSchema).max(6).refine(xs=>uniquePlayerValues(xs,x=>x.color))}),
  z.strictObject({kind:z.literal("impulse-cast"),spell:playerHandleSchema,targetRequired:z.boolean(),targets,generic:rulesCountSchema,
    allowsX:z.boolean().optional(),allowsAlternative:z.boolean().optional(),alternativeTargetless:z.boolean().optional(),alternativePrice:z.strictObject({generic:rulesCountSchema,required:z.array(playerManaSpendSchema).max(6).refine(xs=>uniquePlayerValues(xs,x=>x.color))}).optional(),
    hybridSymbols:z.array(z.strictObject({first:playerManaColorSchema,alternateColor:playerManaColorSchema.nullable()}).refine(s=>s.first!=="colourless" && s.alternateColor!=="colourless" && s.alternateColor!==s.first)).max(PLAYER_ITEMS).optional(),
    fixedTargets:z.array(z.array(playerTargetSchema).max(PLAYER_ITEMS)).max(PLAYER_ITEMS).optional(),
    permanentTargets:z.strictObject({minimum:z.number().int().min(0).max(65535),maximum:z.number().int().min(0).max(65535),targets}).optional(),
    modes:z.strictObject({entwineIndex:z.number().int().nonnegative().optional(),minimum:z.number().int().min(0).max(255),maximum:z.number().int().min(0).max(255),allowRepeat:z.boolean(),
      modes:z.array(z.strictObject({multiple:z.tuple([z.number().int().min(0),z.number().int().min(0)]).optional(),index:z.number().int().min(0).max(255),targetRequired:z.boolean(),targets})).max(PLAYER_ITEMS)}).optional(),
    required:z.array(playerManaSpendSchema).max(6).refine(xs=>uniquePlayerValues(xs,x=>x.color))}),
  z.strictObject({kind:z.enum(["suspended-cast","rebound-cast","cascade-cast"]),spell:playerHandleSchema,targetRequired:z.boolean(),targets}),
  z.strictObject({
    kind: z.literal("library-search"),
    sharedLandType: z.boolean().optional(),
    forestAndPlains: z.boolean().optional(),
    maximum: z.number().int().nonnegative().optional(),
    mode: z.enum(["single", "look-top", "explore", "divide-pile", "battlefield-then-hand", "battlefield-timestamp-order", "hand-batch", "exile-batch", "hand-graveyard"]).optional(),
    entryColorOffers: z.array(z.strictObject({
      object: playerHandleSchema,
      colors: z.array(playerManaColorSchema.exclude(["colourless"])).min(1).max(5).refine((xs) => uniquePlayerValues(xs, (x) => x)),
    })).max(PLAYER_ITEMS).refine((xs) => uniquePlayerValues(xs, (x) => x.object)).optional(),
    entryRevealOffers: z.array(z.strictObject({ object: playerHandleSchema, cards: handles })).max(PLAYER_ITEMS).refine(xs => uniquePlayerValues(xs, x => x.object)).optional(),
    required:z.boolean().optional(),
    inspected: z
      .array(playerDisplayTextSchema(1024).refine((name) => name.length > 0))
      .max(PLAYER_ITEMS),
    eligible: options,
  }),
  z.strictObject({
    kind: z.literal("options"),
    options,
    query: playerHandleSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("objects"),
    priorChoices:z.array(z.strictObject({seatId:z.uuid(),object:playerHandleSchema})).max(6).refine(xs=>uniquePlayerValues(xs,x=>x.seatId)).optional(),
    priorVotes:z.array(z.strictObject({seatId:z.uuid(),object:playerHandleSchema})).optional(),
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
    manaSplit: z.tuple([playerManaColorSchema, playerManaColorSchema]).optional(),
  }),
  z.strictObject({ kind: z.literal("target-list"), targets, minimum: rulesCountSchema, maximum: rulesCountSchema }),
  z.strictObject({
    kind: z.literal("counter-removal"),
    permanent: playerHandleSchema,
    maximum: z.number().int().min(1).max(255),
    available: playerCounterQuantitiesSchema,
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
    upTo: z.boolean().optional(),
  }),
  z.strictObject({
    kind: z.literal("distribution"),
    targets,
    required: rulesCountSchema,
    minimumPerTarget: rulesCountSchema,
  }),
  z.strictObject({
    kind: z.literal("attack-cost"),
    generic: rulesCountSchema,
    whitePhyrexian: rulesCountSchema,
    activations: z.array(z.strictObject({
      id: z.number().int().nonnegative(),
      source: playerHandleSchema.nullable(),
      reversible: z.boolean(),
      dependsOn: z.array(z.number().int().nonnegative()).max(PLAYER_ITEMS),
    })).max(PLAYER_ITEMS).optional(),
  }),
  z.strictObject({
    kind: z.literal("attackers"),
    prices:z.array(z.strictObject({defender:playerTargetSchema,generic:rulesCountSchema,whitePhyrexian:rulesCountSchema})).max(PLAYER_ITEMS).optional(),
    cappedDefenders: z.array(identifierSchema).max(6).refine(xs=>uniquePlayerValues(xs,id=>id)).optional(),
    singleAttacker: z.boolean().optional(),
    choices: z
      .array(attacker)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.attacker)),
    required: handles,
  }),
  z.strictObject({
    kind: z.literal("blockers"),
    defendingSeatId: identifierSchema.optional(),
    singleBlocker: z.boolean().optional(),
    choices: z
      .array(blocker)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.blocker)),
    required: handles,
    minimumIfBlocked: z
      .array(
        z.strictObject({
          attacker: playerHandleSchema,
          minimum: rulesCountSchema,
        }),
      )
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.attacker))
      .optional(),
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
    case "trigger-modes":
      return v.minimum <= v.maximum && v.maximum <= v.modes.length
        && uniquePlayerValues(v.modes, (mode) => String(mode.index))
        && v.modes.every((mode) => {
          if (!mode.multiple) return mode.targetRequired === (mode.targets.length > 0);
          const [minimum, maximum] = mode.multiple;
          return mode.targetRequired && minimum <= maximum && maximum <= mode.targets.length;
        });
    case "impulse-cast": return (v.targetRequired ? (v.targets.length>0 || !!v.alternativeTargetless) : v.targets.length===0) && (!v.alternativeTargetless || !!v.allowsAlternative) && (!!v.alternativePrice === !!v.alternativeTargetless) && !(v.modes && v.permanentTargets) && (!v.fixedTargets || (!v.modes && !v.permanentTargets && !v.targetRequired && v.targets.length===0)) && (!v.permanentTargets || (!v.targetRequired && v.targets.length === 0 && v.permanentTargets.minimum <= v.permanentTargets.maximum)) && (!v.modes || (!v.targetRequired && v.targets.length === 0 && v.modes.minimum <= v.modes.maximum && uniquePlayerValues(v.modes.modes,x=>String(x.index)) && v.modes.modes.every(m=>m.targetRequired || m.targets.length===0)));
    case "madness-cast":
    case "cascade-cast":
    case "rebound-cast":
    case "suspended-cast": return v.targetRequired === (v.targets.length>0);
    case "library-search":
      return v.eligible.every((option) => v.inspected.includes(option.label)) &&
        (v.entryColorOffers ?? []).every((offer) => v.eligible.some((option) => option.handle === offer.object)) &&
        (v.entryRevealOffers ?? []).every((offer) => v.eligible.some((option) => option.handle === offer.object));
    case "options":
      return v.options.length > 0 || v.query !== null;
    case "target-list": return BigInt(v.minimum) <= BigInt(v.maximum) && BigInt(v.maximum) <= BigInt(v.targets.length);
    case "objects":
      return (
        (v.priorVotes ?? []).every(vote=>v.objects.includes(vote.object)) &&
        BigInt(v.minimum) <= BigInt(v.maximum) &&
        BigInt(v.maximum) <= BigInt(v.objects.length)
      );
    case "ordering":
      return (
        v.labels.length === v.objects.length &&
        v.labels.every((entry) => v.objects.includes(entry.handle))
      );
    case "counter-removal":
      return v.available.length > 0 && v.available.every((entry) => entry.count <= v.maximum);
    case "number":
      return BigInt(v.minimum) <= BigInt(v.maximum) &&
        (v.manaSplit === undefined || (v.manaSplit[0] !== v.manaSplit[1] && BigInt(v.minimum) === 0n && BigInt(v.maximum) > 0n));
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
    case "attack-cost": return BigInt(v.generic) + BigInt(v.whitePhyrexian) > 0n &&
      (v.activations ?? []).every((a, i) => a.id === i && new Set(a.dependsOn).size === a.dependsOn.length &&
        a.dependsOn.every(id => id < a.id));
    case "attackers":
      return v.required.every((x) => v.choices.some((c) => c.attacker === x));
    case "blockers":
      return (
        v.required.every((x) => v.choices.some((c) => c.blocker === x)) &&
        (v.minimumIfBlocked ?? []).every(
          (group) =>
            BigInt(group.minimum) > 1n &&
            BigInt(group.minimum) <=
              BigInt(
                v.choices.filter(
                  (row) =>
                    BigInt(row.maximum) > 0n &&
                    row.attackers.includes(group.attacker),
                ).length,
              ),
        )
      );
    case "prevention":
      return total(v.attempts.map((x) => x.maximum)) >= BigInt(v.required);
    default:
      return true;
  }
}
const singleDomain = rawDomain.refine(validDomain);
const castFace = singleDomain.refine((d): d is Extract<RawDomain, { spell: string }> =>
  ["impulse-cast", "madness-cast", "cascade-cast", "rebound-cast", "suspended-cast"].includes(d.kind));
export const playerPromptCategorySchema = z.enum([
  "generic",
  "modal",
  "villainous-choice",
  "dilemma",
  "vote",
  "pile-split",
  "search",
  "tribute",
  "clash",
  "custom",
]);
export const playerPromptDomainSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("objects"),
    candidates: z.array(playerHandleSchema).max(PLAYER_ITEMS).refine((xs) => uniquePlayerValues(xs, (x) => x)),
    minimum: rulesCountSchema,
    maximum: rulesCountSchema,
    hidden: z.boolean(),
  }),
  z.strictObject({
    kind: z.literal("players"),
    seats: z.array(identifierSchema).max(6).refine((xs) => uniquePlayerValues(xs, (x) => x)),
    minimum: z.number().int().min(0).max(6),
    maximum: z.number().int().min(0).max(6),
  }),
  z.strictObject({
    kind: z.literal("options"),
    options: z.array(playerOptionSchema).max(PLAYER_ITEMS),
    minimum: z.number().int().min(0).max(255),
    maximum: z.number().int().min(0).max(255),
    allowRepeat: z.boolean(),
  }),
  z.strictObject({
    kind: z.literal("number"),
    minimum: rulesIntegerSchema,
    maximum: rulesIntegerSchema,
  }),
  z.strictObject({
    kind: z.literal("card-name"),
    nonlandOnly: z.boolean(),
  }),
  z.strictObject({
    kind: z.literal("partition"),
    candidates: z.array(playerHandleSchema).max(PLAYER_ITEMS).refine((xs) => uniquePlayerValues(xs, (x) => x)),
    pileCount: z.number().int().min(2).max(16),
    minPerPile: z.number().int().min(0).max(255),
  }),
]);
export const playerDomainSchema = z.union([singleDomain,z.strictObject({
  kind:z.literal("split-cast"),left:castFace.nullable(),right:castFace,
}).refine(d=>d.left===null || (d.left.kind===d.right.kind && d.left.spell===d.right.spell)), z.strictObject({
  kind: z.literal("adventure-cast"), normal: castFace.nullable(), adventure: castFace,
}).refine(d => d.normal === null || (d.normal.kind === d.adventure.kind &&
  "spell" in d.normal && "spell" in d.adventure && d.normal.spell === d.adventure.spell)),
  z.strictObject({
    kind: z.literal("prompt-choice"),
    category: playerPromptCategorySchema,
    prompt: playerPromptDomainSchema,
  }),
]);
export const playerDecisionKindSchema = z.enum([
  "library-search",
  "select-resolution",
  "entry-reveal",
  "linked-return-order",
  "linked-aura-choice",
  "cast-recovery",
  "order-resolution-graveyard",
  "optional-effect",
  "zone-replacement",
  "resolution-payment",
  "transform-order",
  "timestamp-order",
  "discard",
  "order-graveyard",
  "return-commanders",
  "keep-legend",
  "order-triggers",
  "planeswalker-type",
  "attack-direction",
  "opponent-choice",
  "replacement-chooser",
  "damage-recipient",
  "damage-source",
  "entry-players",
  "myriad-defender",
  "reselect-attack-defender",
  "token-defender",
  "trigger-target",
  "proliferate",
  "counter-removal",
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
  "prompt-choice",
]);
const utcMicrosecond = z.string().refine((v) => {
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
  subjectLabel: playerDisplayTextSchema(1024).refine((label) => label.length > 0).optional(),
  basis: playerBasisSchema,
  respondingSeatIds: z
    .array(identifierSchema)
    .min(1)
    .max(6)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
  visibility: z.enum(["public", "responding-seat", "sealed-until-complete"]),
  kind: playerDecisionKindSchema,
  domain: playerDomainSchema,
  deadlineAt: utcMicrosecond.nullable(),
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
    case "split-cast": case "adventure-cast": return kind === "optional-effect";
    case "trigger-modes":
      return kind === "modes";
    case "impulse-cast":
    case "madness-cast":
    case "cascade-cast":
    case "rebound-cast":
    case "suspended-cast": return kind==="optional-effect";
    case "library-search":
      return kind === "library-search";
    case "options":
      return [
        "planeswalker-type",
        "attack-direction",
        "optional-effect",
  "zone-replacement",
        "mulligan",
        "copy-targets",
        "damage-order",
        "damage-source",
        "mana-color",
        "creature-type",
        "modes",
        "card-name",
      ].includes(kind);
    case "target-list": return kind === "entry-players" || kind === "myriad-defender" || kind === "reselect-attack-defender" || kind === "trigger-target" || kind === "proliferate";
    case "objects":
      return kind === "trigger-target" || (
        (kind === "return-commanders" && domain.minimum === "0" && domain.objects.length > 0 && BigInt(domain.maximum) === BigInt(domain.objects.length)) ||
        kind === "discard" ||
        (kind === "entry-reveal" && domain.minimum === "0" && BigInt(domain.maximum) === BigInt(Math.min(1, domain.objects.length))) ||
        (kind === "bottom" && domain.minimum === domain.maximum) ||
        (kind === "select-resolution" && BigInt(domain.maximum) > 0n)
      );
    case "ordering":
      return (
        ["transform-order","timestamp-order", "order-graveyard", "order-triggers", "linked-return-order"].includes(
          kind,
        ) ||
        (kind === "order-resolution-graveyard" && domain.objects.length > 1)
      );
    case "number":
      return kind === "number";
    case "counter-removal":
      return kind === "counter-removal";
    case "targets":
      return kind === "trigger-target" || kind === "token-defender" || kind === "opponent-choice" || kind === "replacement-chooser" || kind === "damage-recipient" || kind === "linked-aura-choice";
    case "legend":
      return kind === "keep-legend";
    case "payment":
      return kind === "resolution-payment" || kind === "cast-recovery";
    case "distribution":
      return kind === "distribution";
    case "prompt-choice":
      return kind === "prompt-choice";
    case "attack-cost":
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
    (![
      "library-search",
      "select-resolution",
      "entry-reveal",
      "order-resolution-graveyard",
    ].includes(v.kind) ||
      v.respondingSeatIds.length === 1) &&
    (!["library-search", "entry-reveal"].includes(v.kind) || v.visibility === "responding-seat") &&
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
    case "prompt-choice": {
      if (r.kind !== "prompt-choice") return false;
      const p = d.prompt;
      const rep = r.reply;
      if (p.kind === "objects" && rep.kind === "objects") {
        return BigInt(rep.objects.length) >= BigInt(p.minimum) && BigInt(rep.objects.length) <= BigInt(p.maximum)
          && uniquePlayerValues(rep.objects, (x) => x)
          && rep.objects.every((h) => p.candidates.includes(h));
      }
      if (p.kind === "players" && rep.kind === "players") {
        return rep.seats.length >= p.minimum && rep.seats.length <= p.maximum
          && uniquePlayerValues(rep.seats, (x) => x)
          && rep.seats.every((s) => p.seats.includes(s));
      }
      if (p.kind === "options" && rep.kind === "options") {
        return rep.options.length >= p.minimum && rep.options.length <= p.maximum
          && (p.allowRepeat || uniquePlayerValues(rep.options, (x) => x))
          && rep.options.every((h) => p.options.some((o) => o.handle === h));
      }
      if (p.kind === "number" && rep.kind === "number") {
        return BigInt(rep.value) >= BigInt(p.minimum) && BigInt(rep.value) <= BigInt(p.maximum);
      }
      if (p.kind === "card-name" && rep.kind === "card-name") {
        return rep.name.trim().length > 0;
      }
      if (p.kind === "partition" && rep.kind === "partition") {
        if (rep.piles.length !== p.pileCount) return false;
        if (!rep.piles.every((pile) => pile.length >= p.minPerPile)) return false;
        const flat = rep.piles.flat();
        return flat.length === p.candidates.length
          && uniquePlayerValues(flat, (x) => x)
          && flat.every((h) => p.candidates.includes(h));
      }
      return false;
    }
    case "split-cast": {
      if (r.kind==="decline") return true;
      const face="face" in r && r.face==="split-right" ? d.right : d.left;
      return face!==null && allows(face,r,extra);
    }
    case "adventure-cast": {
      if (r.kind === "decline") return true;
      const face = "adventure" in r && r.adventure ? d.adventure : d.normal;
      return face !== null && allows(face, r, extra);
    }
    case "trigger-modes":
      if (r.kind !== "trigger-modes" || r.modes.length < d.minimum || r.modes.length > d.maximum) return false;
      return (!d.distinctTargets || uniquePlayerValues(r.modes.flatMap((mode) => mode.target === null ? (mode.targets ?? []) : [mode.target]), playerTargetKey))
        && r.modes.every((choice) => {
          const mode = d.modes.find((offered) => offered.index === choice.mode);
          if (!mode) return false;
          const selected = choice.targets ?? [];
          if (mode.multiple) {
            const [minimum, maximum] = mode.multiple;
            return choice.target === null && selected.length >= minimum && selected.length <= maximum
              && selected.every((value) => hasTarget(mode.targets, value));
          }
          return selected.length === 0 && (choice.target === null ? !mode.targetRequired : mode.targetRequired && hasTarget(mode.targets, choice.target));
        });
    case "impulse-cast": {
      if (r.kind === "decline") return true;
      if (r.kind !== "impulse-cast" || (r.chosenX !== undefined && !d.allowsX) || (r.useAlternative && !d.allowsAlternative)) return false;
      const modes = r.modes ?? [];
      const permanentTargets = r.permanentTargets ?? [];
      const fixedTargets = r.fixedTargets ?? [];
      const hybridCount = r.hybridChoices?.length ?? 0;
      if (hybridCount && (r.useAlternative || hybridCount !== d.hybridSymbols?.length)) return false;
      if (r.useAlternative && d.alternativeTargetless) return r.target === null && modes.length === 0 && permanentTargets.length === 0 && fixedTargets.length === 0;
      if (d.fixedTargets) return r.target === null && modes.length === 0 && permanentTargets.length === 0 && fixedTargets.length === d.fixedTargets.length && fixedTargets.every((target,i)=>hasTarget(d.fixedTargets![i]!, target));
      if (fixedTargets.length > 0) return false;
      if (d.permanentTargets) return r.target === null && modes.length === 0 && permanentTargets.length >= d.permanentTargets.minimum && permanentTargets.length <= d.permanentTargets.maximum && uniquePlayerValues(permanentTargets,playerTargetKey) && permanentTargets.every(target=>hasTarget(d.permanentTargets!.targets,target));
      if (permanentTargets.length > 0) return false;
      if (!d.modes) return modes.length === 0 && (r.target === null ? !d.targetRequired : d.targetRequired && hasTarget(d.targets,r.target));
      const entwined = (r.additional ?? []).some((cost) => cost.index === d.modes?.entwineIndex && BigInt(cost.times) === 1n);
      const min = entwined ? d.modes.modes.length : d.modes.minimum;
      const max = entwined ? d.modes.modes.length : d.modes.maximum;
      if (r.target !== null || modes.length < min || modes.length > max || (!d.modes.allowRepeat && !uniquePlayerValues(modes,x=>String(x.mode)))) return false;
      return modes.every(choice => { const mode = d.modes?.modes.find(m=>m.index===choice.mode); if (!mode) return false; const selected=choice.targets ?? [];
        if (mode.multiple) return choice.target===null && selected.length>=mode.multiple[0] && selected.length<=mode.multiple[1] && selected.every((target)=>hasTarget(mode.targets,target));
        return selected.length===0 && (choice.target===null ? !mode.targetRequired : mode.targetRequired && hasTarget(mode.targets,choice.target)); });
    }
    case "madness-cast":
    case "cascade-cast":
    case "rebound-cast":
    case "suspended-cast": return r.kind==="decline" || (r.kind===(d.kind==="suspended-cast"?"suspend-cast":d.kind) && (r.target===null ? !d.targetRequired : d.targetRequired && hasTarget(d.targets,r.target)));
    case "library-search": {
      if (d.mode === "hand-graveyard") {
        if (r.kind !== "search-hand-graveyard") return false;
        const chosen = [r.hand, r.graveyard].filter((value): value is string => value !== null);
        return chosen.length === Math.min(2, d.eligible.length) && (chosen.length === 0 || r.hand !== null) && uniquePlayerValues(chosen, (value) => value) && chosen.every((value) => d.eligible.some((option) => option.handle === value));
      }
      if (r.kind !== "objects" && r.kind !== "search-with-entry-colors" && r.kind !== "search-with-entry-choices") return false;
      const colors = r.kind === "objects" ? [] : r.colors;
      const reveals = r.kind === "search-with-entry-choices" ? r.reveals : [];
      const entering = d.mode === "battlefield-then-hand" ? r.objects.slice(0, 1) : r.objects;
      const offers = d.entryColorOffers ?? [];
      const revealOffers = d.entryRevealOffers ?? [];
      return (!d.required || r.objects.length > 0) &&
        r.objects.length <= (d.mode === "divide-pile" ? 5 : (d.mode === "hand-batch" || d.mode === "exile-batch") ? (d.maximum ?? 3) : d.mode && d.mode !== "single" && d.mode !== "look-top" && d.mode !== "explore" ? 2 : 1) &&
        r.objects.every((h) => d.eligible.some((option) => option.handle === h)) &&
        colors.every((choice) => entering.includes(choice.object) && offers.some((offer) => offer.object === choice.object && offer.colors.some((color) => color === choice.color))) &&
        reveals.every((choice) => entering.includes(choice.object) && revealOffers.some((offer) => offer.object === choice.object && (choice.card === null || offer.cards.includes(choice.card))));
    }
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
      return (
        r.kind === "ordering" &&
        r.ordered.length === d.objects.length &&
        r.ordered.every((x) => d.objects.includes(x))
      );
    case "counter-removal":
      return r.kind === "counter-removal"
        && r.counters.reduce((total, entry) => total + entry.count, 0) <= d.maximum
        && r.counters.every((entry) => d.available.some((available) => available.kind === entry.kind && available.count >= entry.count));
    case "number":
      return (
        r.kind === "number" &&
        BigInt(r.value) >= BigInt(d.minimum) &&
        BigInt(r.value) <= BigInt(d.maximum)
      );
    case "target-list": return r.kind === "target-list" && BigInt(r.targets.length) >= BigInt(d.minimum) && BigInt(r.targets.length) <= BigInt(d.maximum) && r.targets.every(value=>hasTarget(d.targets,value));
    case "targets":
      return r.kind === "target" && hasTarget(d.targets, r.target);
    case "legend":
      return (
        r.kind === "legend" &&
        d.conflicts.some(
          (x) => x.conflict === r.conflict && x.objects.includes(r.keep),
        )
      );
    case "attack-cost": {
      if (r.kind === "decline" || r.kind === "redeclare-keeping-mana") return true;
      if (r.kind === "redeclare-reversing-mana") {
        const issued = d.activations ?? [];
        const selected = new Set(r.activations);
        return selected.size === r.activations.length && r.activations.every(id => issued[id]?.reversible) &&
          issued.every(a => selected.has(a.id) || a.dependsOn.every(id => !selected.has(id)));
      }
      if (r.kind !== "pay-attack-cost") return false;
      const white=BigInt(d.whitePhyrexian) - BigInt(r.lifeSymbols);
      return white >= 0n && manaAmount(r.payment.spend,"white") >= white && total(r.payment.spend.map(x=>x.amount)) === BigInt(d.generic) + white;
    }
    case "payment":
      return r.kind === "decline"
        ? d.allowDecline
        : r.kind === "pay" &&
            (d.upTo ? total(r.payment.spend.map(x=>x.amount)) <= BigInt(d.total) : total(r.payment.spend.map(x=>x.amount)) === BigInt(d.total)) &&
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
        [...new Set(r.assignments.flatMap(a => a.band === undefined ? [] : [a.band]))].every(band => {
          const members = r.assignments.filter(a => a.band === band);
          const banders = members.filter(a => d.choices.some(c => c.attacker === a.attacker && c.banding));
          const qualified = bandQualities.some(quality =>
            members.every(a => d.choices.some(c => c.attacker === a.attacker && c.bandQualities?.includes(quality))) &&
            members.some(a => d.choices.some(c => c.attacker === a.attacker && c.bandsWithOther?.includes(quality))));
          return (qualified || (banders.length > 0 && members.length - banders.length <= 1)) &&
            members.every(a => playerTargetKey(a.defender) === playerTargetKey(members[0]!.defender));
        }) &&
        (d.cappedDefenders ?? []).every(id=>r.assignments.filter(a=>a.defender.kind === "seat" && a.defender.seatId === id).length<=1) &&
        (d.singleAttacker
          ? r.assignments.length <= 1 && (d.required.length === 0 || r.assignments.some((a) => d.required.includes(a.attacker)))
          : d.required.every((x) => r.assignments.some((a) => a.attacker === x))) &&
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
        (!d.singleBlocker || r.assignments.filter((a) => a.attackers.length > 0).length <= 1) &&
        d.required.every((x) => r.assignments.some((a) => a.blocker === x)) &&
        (d.minimumIfBlocked ?? []).every((group) => {
          const assigned = r.assignments.filter((row) =>
            row.attackers.includes(group.attacker),
          ).length;
          return assigned === 0 || BigInt(assigned) >= BigInt(group.minimum);
        }) &&
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
        d.sources.every((s) => {
          const assigned = r.assignments.filter(
            (row) => row.source === s.source,
          );
          return (
            total(assigned.map((row) => row.amount)) === BigInt(s.total) &&
            (!assigned.some(
              (row) => row.target.kind === "seat" && BigInt(row.amount) > 0n,
            ) ||
              (s.trampleLethal ?? []).every(
                (minimum) =>
                  BigInt(minimum.amount) === 0n ||
                  assigned.some(
                    (row) =>
                      row.target.kind === "object" &&
                      row.target.handle === minimum.blocker &&
                      BigInt(row.amount) >= BigInt(minimum.amount),
                  ),
              ))
          );
        })
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
  if ("face" in command.action.response && command.action.response.face==="split-right" && v.domain.kind!=="split-cast") return false;
  if ("adventure" in command.action.response && command.action.response.adventure && v.domain.kind !== "adventure-cast") return false;
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
    case "split-cast": return (!d.domain.left || playerDecisionReferences({...d,domain:d.domain.left},objects,stack,roster)) && playerDecisionReferences({...d,domain:d.domain.right},objects,stack,roster);
    case "adventure-cast": return (!d.domain.normal || playerDecisionReferences({...d, domain:d.domain.normal}, objects, stack, roster))
      && playerDecisionReferences({...d, domain:d.domain.adventure}, objects, stack, roster);
    case "counter-removal":
      return objects.has(d.domain.permanent);
    case "trigger-modes":
      return d.domain.modes.every((mode) => mode.targets.every(target));
    case "impulse-cast": return objects.has(d.domain.spell) && d.domain.targets.every(target) && (d.domain.fixedTargets?.every(values=>values.every(target)) ?? true) && (d.domain.modes?.modes.every(mode=>mode.targets.every(target)) ?? true) && (d.domain.permanentTargets?.targets.every(target) ?? true);
    case "madness-cast":
    case "cascade-cast":
    case "rebound-cast":
    case "suspended-cast": return objects.has(d.domain.spell) && d.domain.targets.every(target);
    case "target-list":
    case "targets":
    case "distribution":
      return d.domain.targets.every(target);
    case "objects":
      return (d.domain.priorChoices ?? []).every(row=>roster.includes(row.seatId) && objects.has(row.object)) && d.domain.objects.every((x) => objects.has(x)) && (d.domain.priorVotes ?? []).every(vote=>roster.includes(vote.seatId));
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
      return (d.domain.cappedDefenders ?? []).every(id=>roster.includes(id)) && d.domain.choices.every(
        (x) => objects.has(x.attacker) && x.defenders.every(target),
      );
    case "blockers":
      return (d.domain.defendingSeatId === undefined || roster.includes(d.domain.defendingSeatId)) && d.domain.choices.every(
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
