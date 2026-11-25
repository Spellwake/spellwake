/** Human pregame lifecycle intent/status, without provider/source authority. */
import { z } from "zod";
import { amountSchema, identifierSchema, instantSchema } from "./primitives.ts";
import { roomConfigurationAckSchema } from "./rooms.ts";
import { validProgressionPage } from "./progression.ts";
import { makeCommandDispositionSchema } from "./command-protocol.ts";

export const aiTierSchema = z.enum(["normal", "advanced"]);
export type AiTier = z.infer<typeof aiTierSchema>;

export const revisionRequestSchema = z.strictObject({
  requestId: identifierSchema,
  expectedRevision: amountSchema,
});
export const bootstrapRequestSchema = revisionRequestSchema.extend({
  deckRevisionId: identifierSchema.nullable(),
});
export const setAiTierRequestSchema = revisionRequestSchema.extend({
  tier: aiTierSchema,
});
export type SetAiTierRequest = z.infer<typeof setAiTierRequestSchema>;
export const setAiDeckRequestSchema = revisionRequestSchema.extend({
  deckRevisionId: identifierSchema.nullable(),
});
export type SetAiDeckRequest = z.infer<typeof setAiDeckRequestSchema>;
export const bootstrapAckSchema = z.strictObject({
  resourceId: identifierSchema,
  epoch: z.literal("1"),
});
const removedAckSchema = z.strictObject({ resourceId: identifierSchema });
export const controlAckSchema = z.union([
  roomConfigurationAckSchema,
  bootstrapAckSchema,
  removedAckSchema,
]);
export const addAiErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ room: z.literal("unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
  z.strictObject({
    queue: z.literal("cancel the claimed room before changing its roster"),
  }),
  z.strictObject({ capacity: z.literal("full") }),
]);
export const bootstrapErrorsSchema = z.union([
  z.strictObject({ slot: z.literal("stale or unavailable") }),
  z.strictObject({ deck: z.literal("unavailable") }),
]);
export const removeAiErrorsSchema = z.union([
  z.strictObject({ slot: z.literal("stale or unavailable") }),
  z.strictObject({
    queue: z.literal("cancel the claimed room before changing its roster"),
  }),
]);
export const unreadyErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ room: z.literal("unavailable") }),
  z.strictObject({ ready: z.literal("unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
]);
export const setAiTierErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ slot: z.literal("stale or unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
  z.strictObject({
    queue: z.literal("cancel the claimed room before changing its roster"),
  }),
]);
export const addAiDispositionSchema = makeCommandDispositionSchema(
  addAiErrorsSchema,
  roomConfigurationAckSchema,
);
export const bootstrapDispositionSchema = makeCommandDispositionSchema(
  bootstrapErrorsSchema,
  bootstrapAckSchema,
);
export const removeAiDispositionSchema = makeCommandDispositionSchema(
  removeAiErrorsSchema,
  removedAckSchema,
);
export const unreadyDispositionSchema = makeCommandDispositionSchema(
  unreadyErrorsSchema,
  roomConfigurationAckSchema,
);
export const setAiTierDispositionSchema = makeCommandDispositionSchema(
  setAiTierErrorsSchema,
  roomConfigurationAckSchema,
);
export const setAiDeckErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ slot: z.literal("stale or unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
  z.strictObject({ deck: z.literal("unavailable") }),
  z.strictObject({
    queue: z.literal("cancel the claimed room before changing its roster"),
  }),
]);
export const setAiDeckDispositionSchema = makeCommandDispositionSchema(
  setAiDeckErrorsSchema,
  roomConfigurationAckSchema,
);

export const aiQuotaStatusSchema = z.strictObject({
  accountId: identifierSchema,
  tierName: z.string().min(1).max(64),
  displayUnlimited: z.boolean(),
  dailyLimit: z.number().int().nonnegative().nullable(),
  usedToday: z.number().int().nonnegative(),
  remainingToday: z.number().int().nonnegative().nullable(),
  normalUnlimited: z.literal(true),
  resetsAt: instantSchema,
});
export type AiQuotaStatus = z.infer<typeof aiQuotaStatusSchema>;
export function validControlAck(
  kind: "add" | "bootstrap" | "remove" | "unready",
  target: string,
  basis: string,
  ack: z.infer<typeof controlAckSchema>,
): boolean {
  if (
    !identifierSchema.safeParse(target).success ||
    !amountSchema.safeParse(basis).success
  )
    return false;
  if (kind === "add")
    return "revision" in ack && BigInt(ack.revision) > BigInt(basis);
  if (kind === "bootstrap") return "epoch" in ack && ack.epoch === "1";
  if (kind === "remove")
    return (
      !("epoch" in ack) && !("revision" in ack) && ack.resourceId === target
    );
  return (
    "revision" in ack &&
    ack.resourceId === target &&
    (ack.revision === basis || BigInt(ack.revision) === BigInt(basis) + 1n)
  );
}
const bindingStatusSchema = z.strictObject({
  id: identifierSchema,
  epoch: amountSchema,
  desiredState: z.literal("running"),
  observedState: z.enum([
    "pending",
    "running",
    "ready",
    "unavailable",
    "unknown",
    "stopped",
  ]),
});
export const aiSlotStatusSchema = z
  .strictObject({
    slotId: identifierSchema,
    seatIndex: amountSchema.refine((s) => BigInt(s) <= 9223372036854775807n),
    aiTier: aiTierSchema.optional().default("normal"),
    isSponsor: z.boolean(),
    isRetired: z.boolean(),
    stopPending: z.boolean(),
    deckRevisionId: identifierSchema.nullable(),
    isReady: z.boolean(),
    binding: bindingStatusSchema.nullable(),
  })
  .refine(
    (s) =>
      (s.isSponsor || s.deckRevisionId === null) &&
      (!s.isRetired || (!s.isReady && s.binding === null)) &&
      (!s.isReady || s.binding?.observedState === "ready"),
  );
export const aiStatusPageSchema = z.strictObject({
  accountId: identifierSchema,
  roomId: identifierSchema,
  items: z.array(aiSlotStatusSchema),
  nextCursor: identifierSchema.nullable(),
});
export function validAiStatusPage(
  page: z.infer<typeof aiStatusPageSchema>,
  owner: string,
  roomId: string,
  after: string | null,
  limit: number,
): boolean {
  return (
    identifierSchema.safeParse(roomId).success &&
    page.roomId === roomId &&
    validProgressionPage(
      { ...page, items: page.items.map((row) => ({ id: row.slotId })) },
      owner,
      after,
      limit,
    )
  );
}
export type AiStatusPage = z.infer<typeof aiStatusPageSchema>;
