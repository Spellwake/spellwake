/** Typed user intent. Parsed handles still require a real trusted native resolver. */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";
import {
  playerBasisSchema,
  playerCardTypeSchema,
  playerHandleSchema,
  playerEnglishLabelSchema,
  rulesCountSchema,
  rulesIntegerSchema,
} from "./player-primitives.ts";
export const PLAYER_ITEMS = 1000;
const playerCounterQuantitySchema = z.strictObject({
  kind: z.enum(["plus-one", "minus-one", "page", "quest", "verse", "suspect", "charge",
    "storage", "lore", "loyalty", "defense", "time", "fade", "age", "indestructible", "corpse", "vow", "blessing", "study", "eyeball", "slime"]),
  count: z.number().int().min(1).max(255),
});
export const playerCounterQuantitiesSchema = z.array(playerCounterQuantitySchema).max(PLAYER_ITEMS)
  .refine((counters) => uniquePlayerValues(counters, (counter) => counter.kind));
export function uniquePlayerValues<T>(
  items: readonly T[],
  key: (value: T) => string,
): boolean {
  return new Set(items.map(key)).size === items.length;
}
export const playerManaColorSchema = z.enum([
  "white",
  "blue",
  "black",
  "red",
  "green",
  "colourless",
]);
export const playerManaSpendSchema = z.strictObject({
  color: playerManaColorSchema,
  amount: rulesCountSchema,
});
export const playerRestrictedManaSpendSchema = z.strictObject({
  color: playerManaColorSchema,
  restriction: z.enum(["planeswalker-spell", "colorless-spell", "commander-spell", "colorless-eldrazi-spell-or-ability", "noncreature-spell", "artifact-spell-or-ability", "creature-spell-of-type", "creature-spell-or-ability-of-type"]),
  creatureType: playerEnglishLabelSchema.optional(),
  amount: rulesCountSchema.refine((v) => v !== "0"),
}).refine((entry) => entry.creatureType === undefined || entry.restriction === "creature-spell-of-type" || entry.restriction === "creature-spell-or-ability-of-type");
export const playerPaymentSchema = z
  .strictObject({ spend: z.array(playerManaSpendSchema).max(6), graveyardPermission: playerHandleSchema.optional(), exileCastType: playerCardTypeSchema.exclude(["land"]).optional(), alternativeCostSource: playerHandleSchema.optional(), restrictedMana: z.array(playerRestrictedManaSpendSchema).max(PLAYER_ITEMS).optional() })
  .refine((p) => uniquePlayerValues(p.spend, (x) => x.color) && uniquePlayerValues(p.restrictedMana ?? [], (x) => `${x.color}:${x.restriction}:${x.creatureType ?? ""}`)
    && (p.restrictedMana ?? []).every((item) => BigInt(item.amount) <= BigInt(p.spend.find((entry) => entry.color === item.color)?.amount ?? "0"))
    && p.spend.every((entry) => (p.restrictedMana ?? []).filter((item) => item.color === entry.color).reduce((sum, item) => sum + BigInt(item.amount), 0n) <= BigInt(entry.amount)));
export const playerObjectTargetSchema = z.strictObject({
  kind: z.literal("object"),
  handle: playerHandleSchema,
});
export const playerStackTargetSchema = z.strictObject({
  kind: z.literal("stack"),
  handle: playerHandleSchema,
});
export const playerSeatTargetSchema = z.strictObject({
  kind: z.literal("seat"),
  seatId: identifierSchema,
});
export const playerTargetSchema = z.discriminatedUnion("kind", [
  playerObjectTargetSchema,
  playerStackTargetSchema,
  playerSeatTargetSchema,
]);
export type PlayerTarget = z.infer<typeof playerTargetSchema>;
export function playerTargetKey(target: PlayerTarget): string {
  return JSON.stringify(
    target.kind === "seat"
      ? [target.kind, target.seatId]
      : [target.kind, target.handle],
  );
}
export const playerTargetAllocationSchema = z.strictObject({
  target: playerTargetSchema,
  amount: rulesCountSchema,
});
export const playerCombatAllocationSchema = z.strictObject({
  source: playerHandleSchema,
  target: playerTargetSchema,
  amount: rulesCountSchema,
});
export const playerAttackerAssignmentSchema = z.strictObject({
  band: z.number().int().min(0).max(4294967295).optional(),
  attacker: playerHandleSchema,
  defender: playerTargetSchema,
});
export const playerBlockerAssignmentSchema = z.strictObject({
  blocker: playerHandleSchema,
  attackers: z
    .array(playerHandleSchema)
    .min(1)
    .max(PLAYER_ITEMS)
    .refine((xs) => uniquePlayerValues(xs, (x) => x)),
});
export const playerPreventionAllocationSchema = z.strictObject({
  attempt: playerHandleSchema,
  amount: rulesCountSchema,
});
const handles = z
  .array(playerHandleSchema)
  .max(PLAYER_ITEMS)
  .refine((xs) => uniquePlayerValues(xs, (x) => x));
const playerCastResource = z.discriminatedUnion("kind", [
  z.strictObject({kind:z.literal("splice"), source:playerHandleSchema, target:playerTargetSchema.optional()}),
  z.strictObject({
    kind: z.literal("convoke"),
    source: playerHandleSchema,
    color: playerManaColorSchema.exclude(["colourless"]).nullable(),
  }),
  z.strictObject({ kind: z.literal("improvise"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("retrace"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("jump-start"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("web-slinging"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("emerge"), source: playerHandleSchema }),
  z.strictObject({kind:z.literal("sneak"),source:playerHandleSchema}),
  z.strictObject({ kind: z.literal("bargain"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("escape"), source: playerHandleSchema }),
  z.strictObject({kind:z.literal("reveal-colorless-creature"), source:playerHandleSchema}),
  z.strictObject({kind:z.literal("tap-for-flashback"), source:playerHandleSchema}),
  z.strictObject({kind:z.literal("tap-for-alternative"), source:playerHandleSchema}),
  z.strictObject({kind:z.literal("sacrifice-for-spell"), source:playerHandleSchema}),
  z.strictObject({kind:z.literal("discard-for-spell"), source:playerHandleSchema}),
  z.strictObject({ kind: z.literal("flashback-sacrifice"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("exile-creature-for-reduction"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("delve"), source: playerHandleSchema }),
]);
export const playerChoiceSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("trigger-modes"),
    modes: z.array(z.strictObject({
      mode: z.number().int().min(0).max(255),
      target: playerTargetSchema.nullable(),
      targets: z.array(playerTargetSchema).max(PLAYER_ITEMS).refine((values) => uniquePlayerValues(values, playerTargetKey)).optional(),
    }).refine((mode) => mode.target === null || (mode.targets?.length ?? 0) === 0)).max(PLAYER_ITEMS).refine((options) => uniquePlayerValues(options, (option) => String(option.mode))),
  }),
  z.strictObject({kind:z.literal("madness-cast"),adventure:z.boolean().optional(),face:z.enum(["split-left","split-right"]).optional(),chosenX:rulesCountSchema.optional(),manaAbilities:z.array(z.strictObject({source:playerHandleSchema,ability:playerHandleSchema,color:playerManaColorSchema.nullable().optional(),payment:playerPaymentSchema.nullable().optional(),bundle:playerPaymentSchema.nullable().optional(),costObjects:handles.optional(),recipientSeatId:identifierSchema.optional()})).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>JSON.stringify([x.source,x.ability,x.costObjects ?? []]))).optional(),target:playerTargetSchema.nullable(),payment:playerPaymentSchema.nullable(),
    resources:z.array(playerCastResource).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>JSON.stringify([x.kind === "splice", x.source]))).optional(),
    additional:z.array(z.strictObject({index:z.number().int().min(0).max(65535),times:rulesCountSchema.refine(v=>v!=="0")})).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>String(x.index))).optional()}),
  z.strictObject({kind:z.literal("impulse-cast"),adventure:z.boolean().optional(),face:z.enum(["split-left","split-right"]).optional(),hybridChoices:z.array(z.enum(["first","second"])).max(PLAYER_ITEMS).optional(),fixedTargets:z.array(playerTargetSchema).max(PLAYER_ITEMS).optional(),permanentTargets:z.array(playerTargetSchema).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,playerTargetKey)).optional(),chosenX:rulesCountSchema.optional(),modes:z.array(z.strictObject({mode:z.number().int().min(0).max(255),target:playerTargetSchema.nullable(),targets:z.array(playerTargetSchema).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,playerTargetKey)).optional()})).max(PLAYER_ITEMS).optional(),useAlternative:z.boolean().optional(),manaAbilities:z.array(z.strictObject({source:playerHandleSchema,ability:playerHandleSchema,color:playerManaColorSchema.nullable().optional(),payment:playerPaymentSchema.nullable().optional(),bundle:playerPaymentSchema.nullable().optional(),costObjects:handles.optional(),recipientSeatId:identifierSchema.optional()})).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>JSON.stringify([x.source,x.ability,x.costObjects ?? []]))).optional(),target:playerTargetSchema.nullable(),payment:playerPaymentSchema.nullable(),
    resources:z.array(playerCastResource).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>JSON.stringify([x.kind === "splice", x.source]))).optional(),
    additional:z.array(z.strictObject({index:z.number().int().min(0).max(65535),times:rulesCountSchema.refine(v=>v!=="0")})).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>String(x.index))).optional()}),
  z.strictObject({kind:z.enum(["suspend-cast","rebound-cast","cascade-cast"]),adventure:z.boolean().optional(),face:z.enum(["split-left","split-right"]).optional(),chosenX:rulesCountSchema.optional(),target:playerTargetSchema.nullable(),payment:playerPaymentSchema.nullable(),
    resources:z.array(playerCastResource).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>JSON.stringify([x.kind === "splice", x.source]))).optional(),
    additional:z.array(z.strictObject({index:z.number().int().min(0).max(65535),times:rulesCountSchema.refine(v=>v!=="0")})).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,x=>String(x.index))).optional()}),
  z.strictObject({ kind: z.literal("option"), option: playerHandleSchema }),
  z.strictObject({ kind: z.literal("objects"), objects: handles }),
  z.strictObject({ kind: z.literal("ordering"), ordered: handles }),
  z.strictObject({ kind: z.literal("number"), value: rulesIntegerSchema }),
  z.strictObject({ kind: z.literal("counter-removal"), counters: playerCounterQuantitiesSchema }),
  z.strictObject({ kind: z.literal("target-list"), targets: z.array(playerTargetSchema).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,playerTargetKey)) }),
  z.strictObject({ kind: z.literal("target"), target: playerTargetSchema }),
  z.strictObject({
    kind: z.literal("search-with-entry-colors"),
    objects: handles,
    colors: z.array(z.strictObject({ object: playerHandleSchema, color: playerManaColorSchema })).max(PLAYER_ITEMS).refine(xs => uniquePlayerValues(xs, x => x.object)),
  }),
  z.strictObject({ kind: z.literal("search-with-entry-choices"), objects: handles,
    colors: z.array(z.strictObject({ object: playerHandleSchema, color: playerManaColorSchema })).max(PLAYER_ITEMS).refine(xs => uniquePlayerValues(xs, x => x.object)),
    reveals: z.array(z.strictObject({ object: playerHandleSchema, card: playerHandleSchema.nullable() })).max(PLAYER_ITEMS).refine(xs => uniquePlayerValues(xs, x => x.object)),
  }),
  z.strictObject({ kind: z.literal("search-hand-graveyard"), hand: playerHandleSchema.nullable(), graveyard: playerHandleSchema.nullable() }).refine((choice) => choice.hand === null || choice.hand !== choice.graveyard),
  z.strictObject({
    kind: z.literal("legend"),
    conflict: playerHandleSchema,
    keep: playerHandleSchema,
  }),
  z.strictObject({kind:z.literal("redeclare-keeping-mana")}),
  z.strictObject({
    kind: z.literal("redeclare-reversing-mana"),
    activations: z.array(z.number().int().nonnegative()).max(PLAYER_ITEMS),
  }),
  z.strictObject({kind:z.literal("pay-attack-cost"),lifeSymbols:rulesCountSchema,payment:playerPaymentSchema})
    .refine(row=>!row.payment.restrictedMana?.length && row.payment.exileCastType === undefined && row.payment.graveyardPermission === undefined && row.payment.alternativeCostSource === undefined),
  z.strictObject({ kind: z.literal("pay"), payment: playerPaymentSchema }),
  z.strictObject({ kind: z.literal("decline") }),
  z.strictObject({
    kind: z.literal("prevention"),
    allocations: z
      .array(playerPreventionAllocationSchema)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.attempt)),
  }),
  z.strictObject({
    kind: z.literal("distribution"),
    allocations: z
      .array(playerTargetAllocationSchema)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => playerTargetKey(x.target))),
  }),
  z.strictObject({
    kind: z.literal("attackers"),
    assignments: z
      .array(playerAttackerAssignmentSchema)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.attacker)),
  }),
  z.strictObject({
    kind: z.literal("blockers"),
    assignments: z
      .array(playerBlockerAssignmentSchema)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => x.blocker)),
  }),
  z.strictObject({
    kind: z.literal("combat-damage"),
    assignments: z
      .array(playerCombatAllocationSchema)
      .max(PLAYER_ITEMS)
      .refine((xs) =>
        uniquePlayerValues(xs, (x) =>
          JSON.stringify([x.source, playerTargetKey(x.target)]),
        ),
      ),
  }),
]).refine(x=>!("face" in x && x.face && "adventure" in x && x.adventure));
export const playerActionSchema = z.discriminatedUnion("kind", [
  z.strictObject({kind:z.literal("cast-face-down"),source:playerHandleSchema,appearance:z.enum(["morph","disguise"]),payment:playerPaymentSchema.nullable()}),
  z.strictObject({kind:z.literal("turn-face-up"),source:playerHandleSchema,ability:z.number().int().min(0).max(65535),payment:playerPaymentSchema.nullable()}),
  z.strictObject({
    kind: z.literal("cast-with-resources"),
    adventure:z.boolean().optional(),face:z.enum(["split-left","split-right","disturb"]).optional(),
    hybridChoices: z.array(z.enum(["first", "second"])).max(PLAYER_ITEMS).optional(),
    fixedTargets: z.array(playerTargetSchema).max(PLAYER_ITEMS).optional(),
    permanentTargets: z.array(playerTargetSchema).max(PLAYER_ITEMS).optional(),
    chosenX: rulesCountSchema.optional(),
  modes: z.array(z.strictObject({ mode: z.number().int().min(0).max(255), target: playerTargetSchema.nullable(), targets: z.array(playerTargetSchema).max(PLAYER_ITEMS).refine(xs=>uniquePlayerValues(xs,playerTargetKey)).optional() })).max(PLAYER_ITEMS).optional(),
    useAlternative: z.boolean().optional(),
    additional: z
      .array(
        z.strictObject({
          index: z.number().int().min(0).max(65535),
          times: rulesCountSchema.refine((value) => value !== "0"),
        }),
      )
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => String(x.index)))
      .optional(),
    source: playerHandleSchema,
    target: playerTargetSchema.nullable(),
    payment: playerPaymentSchema.nullable(),
    resources: z
      .array(playerCastResource)
      .max(PLAYER_ITEMS)
      .refine((xs) => uniquePlayerValues(xs, (x) => JSON.stringify([x.kind === "splice", x.source]))),
  }),
  z.strictObject({kind:z.literal("plot"),source:playerHandleSchema,payment:playerPaymentSchema.nullable()}),
  z.strictObject({kind:z.literal("foretell"),source:playerHandleSchema,payment:playerPaymentSchema.nullable()}),
  z.strictObject({kind:z.literal("suspend"),source:playerHandleSchema,payment:playerPaymentSchema.nullable()}),
  z.strictObject({
    kind: z.literal("cast-untargeted"),
    source: playerHandleSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("cast-targeted"),
    source: playerHandleSchema,
    target: playerTargetSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("cycle"),
    source: playerHandleSchema,
    ability: playerHandleSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("activate-with-creatures"),
    source: playerHandleSchema, ability: playerHandleSchema, creatures: z.array(playerHandleSchema).max(PLAYER_ITEMS).refine((v) => new Set(v).size === v.length),
  }),
  z.strictObject({ kind: z.literal("activate-with-cost-objects"), source: playerHandleSchema, ability: playerHandleSchema, target: playerTargetSchema.nullable(), objects: handles, payment: playerPaymentSchema.nullable() }),
  z.strictObject({
    kind: z.literal("activate-untargeted-ability"),
    source: playerHandleSchema,
    ability: playerHandleSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({kind:z.literal("ninjutsu"),source:playerHandleSchema,ability:playerHandleSchema,attacker:playerHandleSchema,payment:playerPaymentSchema.nullable()}),
  z.strictObject({
    kind: z.literal("transmute"),
    source: playerHandleSchema,
    ability: playerHandleSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("activate-emblem"),
    emblemIndex: z.number().int().min(0).max(0xffffffff),
    creature: playerHandleSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("activate-permanent-targets"),
    source: playerHandleSchema, ability: playerHandleSchema,
    targets: z.array(playerTargetSchema).max(PLAYER_ITEMS), payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("activate-untargeted-x"),
    source: playerHandleSchema,
    ability: playerHandleSchema,
    chosenX: rulesCountSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("activate-loyalty-x"),
    source: playerHandleSchema,
    ability: playerHandleSchema,
    target: playerTargetSchema,
    chosenX: rulesCountSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("activate-ability"),
    source: playerHandleSchema,
    ability: playerHandleSchema,
    target: playerTargetSchema,
    payment: playerPaymentSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("activate-mana"),
    source: playerHandleSchema,
    ability: playerHandleSchema,
  }),
  z.strictObject({ kind: z.literal("activate-mana-with-payment"), source: playerHandleSchema, ability: playerHandleSchema, payment: playerPaymentSchema }),
  z.strictObject({ kind: z.literal("activate-mana-with-cost-objects"), source: playerHandleSchema, ability: playerHandleSchema, objects: handles }),
  z.strictObject({ kind: z.literal("activate-hybrid-mana"), source: playerHandleSchema, ability: playerHandleSchema, payment: playerPaymentSchema, bundle: playerPaymentSchema }),
  z.strictObject({ kind: z.literal("activate-mana-for-player"), source: playerHandleSchema, ability: playerHandleSchema, recipientSeatId: identifierSchema }),
  z.strictObject({ kind: z.literal("activate-mana-with-bundle"), source: playerHandleSchema, ability: playerHandleSchema, bundle: playerPaymentSchema }),
  z.strictObject({ kind: z.literal("play-land"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("play-land-with-entry-color"), source: playerHandleSchema, color: playerManaColorSchema.exclude(["colourless"]) }),
  z.strictObject({ kind: z.literal("play-land-with-entry-reveal"), source: playerHandleSchema, reveal: playerHandleSchema.nullable() }),
  z.strictObject({ kind: z.literal("pass") }),
  z.strictObject({
    kind: z.literal("choose"),
    decision: playerHandleSchema,
    response: playerChoiceSchema,
  }),
  z.strictObject({ kind: z.literal("concede") }),
]).refine(x=>!(x.kind==="cast-with-resources" && x.face && x.adventure));
export const playerCommandSchema = z.strictObject({
  requestId: identifierSchema,
  basis: playerBasisSchema,
  action: playerActionSchema,
});
export type PlayerCommand = z.infer<typeof playerCommandSchema>;
export type PlayerChoice = z.infer<typeof playerChoiceSchema>;
export type PlayerPayment = z.infer<typeof playerPaymentSchema>;
