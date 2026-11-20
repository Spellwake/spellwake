/** Typed user intent. Parsed handles still require a real trusted native resolver. */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";
import {
  playerBasisSchema,
  playerHandleSchema,
  rulesCountSchema,
  rulesIntegerSchema,
} from "./player-primitives.ts";
export const PLAYER_ITEMS = 1000;
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
export const playerPaymentSchema = z
  .strictObject({ spend: z.array(playerManaSpendSchema).max(6) })
  .refine((p) => uniquePlayerValues(p.spend, (x) => x.color));
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
export const playerChoiceSchema = z.discriminatedUnion("kind", [
  z.strictObject({kind:z.literal("library-partition"),top:handles,other:handles})
    .refine(x => x.top.length + x.other.length <= PLAYER_ITEMS && uniquePlayerValues([...x.top,...x.other], value => value)),
  z.strictObject({ kind: z.literal("option"), option: playerHandleSchema }),
  z.strictObject({ kind: z.literal("objects"), objects: handles }),
  z.strictObject({ kind: z.literal("ordering"), ordered: handles }),
  z.strictObject({ kind: z.literal("number"), value: rulesIntegerSchema }),
  z.strictObject({ kind: z.literal("target"), target: playerTargetSchema }),
  z.strictObject({
    kind: z.literal("legend"),
    conflict: playerHandleSchema,
    keep: playerHandleSchema,
  }),
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
]);
export const playerActionSchema = z.discriminatedUnion("kind", [
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
  z.strictObject({ kind: z.literal("play-land"), source: playerHandleSchema }),
  z.strictObject({ kind: z.literal("pass") }),
  z.strictObject({
    kind: z.literal("choose"),
    decision: playerHandleSchema,
    response: playerChoiceSchema,
  }),
  z.strictObject({ kind: z.literal("concede") }),
]);
export const playerCommandSchema = z.strictObject({
  requestId: identifierSchema,
  basis: playerBasisSchema,
  action: playerActionSchema,
});
export type PlayerCommand = z.infer<typeof playerCommandSchema>;
export type PlayerChoice = z.infer<typeof playerChoiceSchema>;
export type PlayerPayment = z.infer<typeof playerPaymentSchema>;
