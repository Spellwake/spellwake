/** Proof-free original human intents. Qualifiers/selectors, retained preparation and
 * authenticated handlers are separate dependencies; these schemas install none.
 */
import { z } from "zod";
import { amountSchema, identifierSchema } from "./primitives.ts";
import { revisionRequestSchema } from "./room-controls.ts";
import { roomConfigurationAckSchema } from "./rooms.ts";
import { makeCommandDispositionSchema } from "./command-protocol.ts";
export const roomAdmissionIntentSchema = revisionRequestSchema;
export const readyRoomErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ deck: z.literal("qualified selected revision required") }),
  z.strictObject({ ready: z.literal("unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
  z.strictObject({ room: z.literal("unavailable") }),
]);
export const forceStartErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ decks: z.literal("qualification required") }),
  z.strictObject({ revision: z.literal("conflict") }),
  z.strictObject({
    room: z.enum(["invalid slots or selected decks", "unavailable"]),
  }),
]);
export const readyRoomDispositionSchema = makeCommandDispositionSchema(
  readyRoomErrorsSchema,
  roomConfigurationAckSchema,
);
export const forceStartDispositionSchema = makeCommandDispositionSchema(
  forceStartErrorsSchema,
  roomConfigurationAckSchema,
);
export const betweenGameIntentSchema = z.strictObject({
  requestId: identifierSchema,
  decisionId: identifierSchema,
  expectedRevision: amountSchema,
  selectedRevisionId: identifierSchema,
});
export const betweenGameAckSchema = z.strictObject({
  resourceId: identifierSchema,
  revision: amountSchema,
  reservedGameId: identifierSchema.nullable(),
});
export const betweenGameErrorsSchema = z.union([
  z.strictObject({
    basis: z.literal("stale or unavailable between-game decision"),
  }),
  z.strictObject({
    deck: z.enum([
      "unavailable selected revision",
      "selection changes the immutable original card pool",
    ]),
  }),
  z.strictObject({ deadline: z.literal("qualified cutoff has elapsed") }),
]);
export const betweenGameDispositionSchema = makeCommandDispositionSchema(
  betweenGameErrorsSchema,
  betweenGameAckSchema,
);
export const questRerollIntentSchema = z.strictObject({
  requestId: identifierSchema,
  assignmentId: identifierSchema,
});
export const questRerollAckSchema = z.strictObject({
  resourceId: identifierSchema,
  replacedId: identifierSchema,
});
export const questRerollErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ day: z.literal("not current UTC day") }),
  z.strictObject({ assignment: z.literal("unavailable") }),
  z.strictObject({
    definition: z.literal("different available definition required"),
  }),
  z.strictObject({ reroll: z.literal("already used") }),
]);
export const questRerollDispositionSchema = makeCommandDispositionSchema(
  questRerollErrorsSchema,
  questRerollAckSchema,
);
export type RoomAdmissionIntent = z.infer<typeof roomAdmissionIntentSchema>;
export type BetweenGameIntent = z.infer<typeof betweenGameIntentSchema>;
export type QuestRerollIntent = z.infer<typeof questRerollIntentSchema>;
/** Original revision may remain unchanged on a qualified no-op. No current state is read. */
export function validRoomAdmissionAck(
  original: RoomAdmissionIntent,
  room: string,
  ack: z.infer<typeof roomConfigurationAckSchema>,
): boolean {
  return (
    roomAdmissionIntentSchema.safeParse(original).success &&
    identifierSchema.safeParse(room).success &&
    roomConfigurationAckSchema.safeParse(ack).success &&
    ack.resourceId === room &&
    (ack.revision === original.expectedRevision ||
      BigInt(ack.revision) === BigInt(original.expectedRevision) + 1n)
  );
}
/** Persisted result validation is separate from authorization and qualifier truth. */
export function validRoomAdmissionDisposition(
  original: RoomAdmissionIntent,
  room: string,
  force: boolean,
  disposition: unknown,
): boolean {
  const parsed = (
    force ? forceStartDispositionSchema : readyRoomDispositionSchema
  ).safeParse(disposition);
  return (
    roomAdmissionIntentSchema.safeParse(original).success &&
    identifierSchema.safeParse(room).success &&
    parsed.success &&
    (parsed.data.state !== "accepted" ||
      validRoomAdmissionAck(original, room, parsed.data.response.data))
  );
}
export function validBetweenGameAck(
  original: BetweenGameIntent,
  ack: z.infer<typeof betweenGameAckSchema>,
): boolean {
  return (
    betweenGameIntentSchema.safeParse(original).success &&
    betweenGameAckSchema.safeParse(ack).success &&
    ack.resourceId === original.decisionId &&
    BigInt(ack.revision) === BigInt(original.expectedRevision) + 1n
  );
}
export function validBetweenGameDisposition(
  original: BetweenGameIntent,
  disposition: unknown,
): boolean {
  const parsed = betweenGameDispositionSchema.safeParse(disposition);
  return (
    betweenGameIntentSchema.safeParse(original).success &&
    parsed.success &&
    (parsed.data.state !== "accepted" ||
      validBetweenGameAck(original, parsed.data.response.data))
  );
}
export function validQuestRerollAck(
  original: QuestRerollIntent,
  ack: z.infer<typeof questRerollAckSchema>,
): boolean {
  return (
    questRerollIntentSchema.safeParse(original).success &&
    questRerollAckSchema.safeParse(ack).success &&
    ack.replacedId === original.assignmentId &&
    ack.resourceId !== original.assignmentId
  );
}
export function validQuestRerollDisposition(
  original: QuestRerollIntent,
  disposition: unknown,
): boolean {
  const parsed = questRerollDispositionSchema.safeParse(disposition);
  return (
    questRerollIntentSchema.safeParse(original).success &&
    parsed.success &&
    (parsed.data.state !== "accepted" ||
      validQuestRerollAck(original, parsed.data.response.data))
  );
}
