/** Immutable economic attempts; saved records never authorize automatic submission. */
import { z } from "zod";
import {
  identifierSchema,
  economyCommandSchema,
  economyCommandErrorsSchema,
  economyCommandDispositionSchema,
  economyAckMatches,
  commandAckSchema,
  type EconomyCommand,
} from "@spellwake/application-contracts";
import { publicOrigin } from "./application-api";
import { createIntentJournal } from "./intent-journal";

export const economySlotSchema = z.strictObject({
  kind: z.enum([
    "trade_create",
    "trade_response",
    "listing_create",
    "listing_cancel",
    "listing_buy",
    "pack_buy",
    "pack_open",
  ]),
  targetId: identifierSchema,
});
export type EconomySlot = z.infer<typeof economySlotSchema>;
/** A response action shares its trade slot; changed terms cannot replace an unresolved attempt. */
export function economyIntentSlot(intent: EconomyCommand): EconomySlot {
  switch (intent.kind) {
    case "trade_create":
      return { kind: intent.kind, targetId: intent.command.recipientId };
    case "trade_response":
      return { kind: intent.kind, targetId: intent.command.tradeId };
    case "listing_create":
      return { kind: intent.kind, targetId: intent.command.lot.assetId };
    case "listing_cancel":
    case "listing_buy":
      return { kind: intent.kind, targetId: intent.command.listingId };
    case "pack_buy":
      return { kind: intent.kind, targetId: intent.command.recipeId };
    case "pack_open":
      return { kind: intent.kind, targetId: intent.command.inventoryAssetId };
  }
}
const scopedSchema = z.strictObject({
  subjectId: identifierSchema,
  intent: economyCommandSchema,
});
export type ScopedEconomyIntent = z.infer<typeof scopedSchema>;

/** Bind exact provider subject, API, account, operation and target before storing any attempt. */
export function createEconomyJournal(
  origin: string,
  accountId: string,
  subjectId: string,
  slotInput: EconomySlot,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  if (publicOrigin(origin) !== origin)
    throw new Error("Economy recovery scope is unavailable.");
  identifierSchema.parse(accountId);
  identifierSchema.parse(subjectId);
  const slot = economySlotSchema.parse(slotInput);
  const key = `spellwake.economy-command.v1:${origin}:${accountId}:${slot.kind}:${slot.targetId}`;
  const commandSchema = scopedSchema.refine((command) => {
    const target = economyIntentSlot(command.intent);
    return (
      command.subjectId === subjectId &&
      target.kind === slot.kind &&
      target.targetId === slot.targetId
    );
  });
  const source = {
    version: z.literal(1),
    apiOrigin: z.literal(origin),
    accountId: z.literal(accountId),
    command: commandSchema,
  };
  const schema = z
    .discriminatedUnion("state", [
      z.strictObject({ ...source, state: z.literal("unresolved") }),
      z.strictObject({
        ...source,
        state: z.literal("accepted"),
        ack: commandAckSchema,
      }),
      z.strictObject({
        ...source,
        state: z.literal("rejected"),
        errors: economyCommandErrorsSchema,
      }),
    ])
    .refine(
      (entry) =>
        entry.state !== "accepted" ||
        economyAckMatches(entry.command.intent, entry.ack.resourceId),
    );
  return {
    ...createIntentJournal<
      ScopedEconomyIntent,
      z.infer<typeof economyCommandErrorsSchema>
    >(
      key,
      origin,
      accountId,
      schema,
      commandSchema,
      economyCommandDispositionSchema,
      (command) => command.intent.command.requestId,
      getStorage,
    ),
    origin,
    accountId,
    subjectId,
    slot,
  };
}
