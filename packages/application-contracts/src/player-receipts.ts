/** Historical original-intent correlation; this module executes no moves. */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";
import { playerBasisSchema, samePlayerWireValue } from "./player-primitives.ts";
import {
  playerCommandSchema,
  uniquePlayerValues,
  type PlayerCommand,
} from "./player-actions.ts";
import { makeCommandDispositionSchema } from "./command-protocol.ts";

export const playerCommandAckSchema = z.strictObject({
  requestId: identifierSchema,
  actorSeatId: identifierSchema,
  basis: playerBasisSchema,
});
/** Only safe public reasons, never native error text or private identifiers. */
export const playerRejectionSchema = z
  .partialRecord(
    z.enum([
      "request",
      "game",
      "basis",
      "actor",
      "source",
      "ability",
      "target",
      "decision",
      "response",
      "payment",
      "action",
    ]),
    z.enum(["invalid", "stale", "unavailable", "not permitted", "unsupported"]),
  )
  .refine((x) => Object.keys(x).length > 0);
export const playerReceiptSchema = z.strictObject({
  actorSeatId: identifierSchema,
  original: playerCommandSchema,
  disposition: makeCommandDispositionSchema(
    playerRejectionSchema,
    playerCommandAckSchema,
  ),
});
export type PlayerReceipt = z.infer<typeof playerReceiptSchema>;
/** Validate against independently retained intent and verified seat, not today's board. */
export function validPlayerReceipt(
  receipt: PlayerReceipt,
  original: PlayerCommand,
  actor: string,
): boolean {
  if (
    !identifierSchema.safeParse(actor).success ||
    !playerCommandSchema.safeParse(original).success ||
    !playerReceiptSchema.safeParse(receipt).success ||
    receipt.actorSeatId !== actor ||
    !samePlayerWireValue(receipt.original, original)
  )
    return false;
  if (receipt.disposition.state !== "accepted") return true;
  const ack = receipt.disposition.response.data;
  return (
    ack.requestId === original.requestId &&
    ack.actorSeatId === actor &&
    ack.basis.gameId === original.basis.gameId &&
    ack.basis.contextId === original.basis.contextId &&
    ack.basis.epoch === original.basis.epoch &&
    BigInt(ack.basis.revision) === BigInt(original.basis.revision) + 1n
  );
}
export const playerReceiptQuerySchema = z
  .strictObject({
    gameId: identifierSchema,
    contextId: identifierSchema,
    outstanding: z.array(playerCommandSchema).max(100),
  })
  .refine(
    (q) =>
      uniquePlayerValues(q.outstanding, (x) => x.requestId) &&
      q.outstanding.every(
        (x) => x.basis.gameId === q.gameId && x.basis.contextId === q.contextId,
      ),
  );
export type PlayerReceiptQuery = z.infer<typeof playerReceiptQuerySchema>;
/** Exactly one result per original intent, in original order; no retry execution. */
export function validPlayerReceipts(
  query: PlayerReceiptQuery,
  receipts: readonly PlayerReceipt[],
  actor: string,
): boolean {
  return (
    identifierSchema.safeParse(actor).success &&
    playerReceiptQuerySchema.safeParse(query).success &&
    receipts.length === query.outstanding.length &&
    query.outstanding.every((original, i) => {
      const receipt = receipts[i];
      return (
        receipt !== undefined && validPlayerReceipt(receipt, original, actor)
      );
    })
  );
}
