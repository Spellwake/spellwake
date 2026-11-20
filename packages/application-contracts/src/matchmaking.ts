/** Matchmaking consent and safe status are separate from native admission and matcher authority. */
import { z } from "zod";
import { identifierSchema, amountSchema } from "./primitives.ts";
import { validProgressionPage } from "./progression.ts";
const keySchema = z
  .string()
  .refine(
    (value) =>
      !value.includes("\0") &&
      !/[\uD800-\uDFFF]/u.test(value) &&
      Array.from(value.replace(/^ +| +$/g, "")).length >= 1 &&
      Array.from(value.replace(/^ +| +$/g, "")).length <= 128,
  );
const aiSeatsSchema = z.string().regex(/^[0-5]$/, { abort: true });
export const queueChoiceSchema = z
  .strictObject({
    profileId: identifierSchema,
    configurationKey: keySchema,
    maximumAiSeats: aiSeatsSchema,
    maximumSponsoredAiSeats: aiSeatsSchema,
  })
  .refine((c) => BigInt(c.maximumSponsoredAiSeats) <= BigInt(c.maximumAiSeats));
export const queueAlternativeSchema = z
  .strictObject({
    deckRevisionId: identifierSchema,
    choices: z.array(queueChoiceSchema).min(1).max(128),
  })
  .refine(
    (a) =>
      new Set(
        a.choices.map((c) => JSON.stringify([c.profileId, c.configurationKey])),
      ).size === a.choices.length,
  );
const intentContentSchema = z
  .strictObject({
    mode: z.enum(["open", "collection"]),
    competitive: z.boolean(),
    alternatives: z.array(queueAlternativeSchema).min(1).max(32),
  })
  .refine(
    (i) =>
      (!i.competitive || i.mode === "collection") &&
      new Set(i.alternatives.map((a) => a.deckRevisionId)).size ===
        i.alternatives.length &&
      (!i.competitive ||
        i.alternatives.every((a) =>
          a.choices.every((c) => c.maximumAiSeats === "0"),
        )),
  );
export const enqueueMatchmakingSchema = intentContentSchema
  .safeExtend({ requestId: identifierSchema })
  .refine((i) => new TextEncoder().encode(JSON.stringify(i)).length <= 264192);
export const cancelMatchmakingSchema = z.strictObject({
  requestId: identifierSchema,
});
export const enqueueMatchmakingAckSchema = z.strictObject({
  resourceId: identifierSchema,
  intentId: identifierSchema,
});
export const enqueueMatchmakingErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ intent: z.literal("invalid envelope") }),
  z.strictObject({
    alternatives: z.literal("distinct bounded decks required"),
  }),
  z.strictObject({ alternative: z.literal("invalid shape") }),
  z.strictObject({
    choices: z.literal("distinct bounded configurations required"),
  }),
  z.strictObject({
    choice: z.enum([
      "invalid shape",
      "unsupported format, profile or AI allowance",
    ]),
  }),
  z.strictObject({ deck: z.literal("owned sealed revision required") }),
]);
export const cancelMatchmakingErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ queue: z.enum(["unavailable", "already reserved"]) }),
]);
export const queueEntrySchema = z
  .strictObject({
    id: identifierSchema,
    intentId: identifierSchema.nullable(),
    deckRevisionId: identifierSchema,
    mode: z.enum(["open", "collection"]),
    competitive: z.boolean(),
    alternativeIndex: amountSchema.nullable(),
    formatId: identifierSchema.nullable(),
    state: z.enum(["queued", "claimed", "reserved", "canceled"]),
    claimedRoomId: identifierSchema.nullable(),
    reservedMatchId: identifierSchema.nullable(),
  })
  .refine((e) => {
    const typed =
      e.intentId === null
        ? e.alternativeIndex === null && e.formatId === null
        : e.alternativeIndex !== null &&
          BigInt(e.alternativeIndex) <= 31n &&
          e.formatId !== null;
    const lifecycle =
      e.state === "queued"
        ? e.claimedRoomId === null && e.reservedMatchId === null
        : e.state === "claimed"
          ? e.claimedRoomId !== null && e.reservedMatchId === null
          : e.state === "reserved"
            ? e.claimedRoomId !== null && e.reservedMatchId !== null
            : e.reservedMatchId === null;
    return typed && lifecycle && (!e.competitive || e.mode === "collection");
  });
export const queuePageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(queueEntrySchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export const queueIntentSchema = intentContentSchema.safeExtend({
  accountId: identifierSchema,
  intentId: identifierSchema,
  requestId: identifierSchema,
});
export const tableProfileSchema = z
  .strictObject({
    id: identifierSchema,
    key: z.string(),
    minimumSeats: amountSchema,
    seatCapacity: amountSchema,
    hasMatchmaking: z.boolean(),
  })
  .refine(
    (p) =>
      BigInt(p.minimumSeats) >= 2n &&
      BigInt(p.minimumSeats) <= BigInt(p.seatCapacity) &&
      BigInt(p.seatCapacity) <= 6n,
  );
export const tableProfilePageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(tableProfileSchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export const profileConfigurationSchema = z
  .strictObject({
    index: amountSchema,
    key: keySchema,
    formatId: identifierSchema,
    seatCount: amountSchema,
    structure: z.enum(["bo1", "bo3"]),
  })
  .refine(
    (c) =>
      BigInt(c.index) <= 511n &&
      BigInt(c.seatCount) >= 2n &&
      BigInt(c.seatCount) <= 6n &&
      (c.structure !== "bo3" || c.seatCount === "2"),
  );
export const profileConfigurationPageSchema = z.strictObject({
  accountId: identifierSchema,
  profileId: identifierSchema,
  items: z.array(profileConfigurationSchema).max(100),
  nextIndex: amountSchema.nullable(),
});
/** Shape parsing precedes caller/intent/page validation. */
export function validQueuePage(
  page: z.infer<typeof queuePageSchema>,
  owner: string,
  intent: string | null,
  after: string | null,
  limit: number,
): boolean {
  return (
    validProgressionPage(page, owner, after, limit) &&
    page.items.every((e) => intent === null || e.intentId === intent)
  );
}
/** This reconstructs immutable consent, not current queue membership. */
export function validQueueIntent(
  value: z.infer<typeof queueIntentSchema>,
  owner: string,
  intent: string,
): boolean {
  return value.accountId === owner && value.intentId === intent;
}
/** Profile publication is SQL metadata, not native game coverage. */
export function validTableProfilePage(
  page: z.infer<typeof tableProfilePageSchema>,
  owner: string,
  after: string | null,
  limit: number,
): boolean {
  return validProgressionPage(page, owner, after, limit);
}
/** Ordinality is stable for each immutable published profile revision. */
export function validProfileConfigurationPage(
  page: z.infer<typeof profileConfigurationPageSchema>,
  owner: string,
  profile: string,
  after: number | null,
  limit: number,
): boolean {
  return (
    identifierSchema.safeParse(owner).success &&
    identifierSchema.safeParse(profile).success &&
    page.accountId === owner &&
    page.profileId === profile &&
    (after === null ||
      (Number.isInteger(after) && after >= 0 && after <= 511)) &&
    Number.isInteger(limit) &&
    limit >= 1 &&
    limit <= 100 &&
    page.items.length <= limit &&
    (after !== null || page.items.length > 0) &&
    page.items.every(
      (item, index) => BigInt(item.index) === BigInt((after ?? -1) + 1 + index),
    ) &&
    page.nextIndex ===
      (page.items.length === limit ? (page.items.at(-1)?.index ?? null) : null)
  );
}
