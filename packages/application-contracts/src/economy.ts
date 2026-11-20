/** Immutable economy intent; prices, ownership, holds and outcomes remain server authority. */
import { z } from "zod";
import { packQuantitySchema } from "./pack-shop.ts";
import {
  amountSchema,
  identifierSchema,
  positiveAmountSchema,
} from "./primitives.ts";

export const assetLineSchema = z.strictObject({
  assetId: identifierSchema,
  quantity: positiveAmountSchema,
});
export const assetBundleSchema = z
  .array(assetLineSchema)
  .max(100)
  .refine(
    (lines) => new Set(lines.map((line) => line.assetId)).size === lines.length,
  );
export const economyCommandSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("trade_create"),
    command: z
      .strictObject({
        requestId: identifierSchema,
        recipientId: identifierSchema,
        give: assetBundleSchema,
        request: assetBundleSchema,
      })
      .refine((command) => command.give.length + command.request.length > 0),
  }),
  z.strictObject({
    kind: z.literal("trade_response"),
    command: z.strictObject({
      requestId: identifierSchema,
      tradeId: identifierSchema,
      action: z.enum(["accept", "reject", "cancel"]),
    }),
  }),
  z.strictObject({
    kind: z.literal("listing_create"),
    command: z.strictObject({
      requestId: identifierSchema,
      lot: assetLineSchema,
      totalPrice: positiveAmountSchema,
    }),
  }),
  z.strictObject({
    kind: z.literal("listing_cancel"),
    command: z.strictObject({
      requestId: identifierSchema,
      listingId: identifierSchema,
    }),
  }),
  z.strictObject({
    kind: z.literal("listing_buy"),
    command: z.strictObject({
      requestId: identifierSchema,
      listingId: identifierSchema,
      expectedRevision: positiveAmountSchema,
      expectedTotal: positiveAmountSchema,
    }),
  }),
  z.strictObject({
    kind: z.literal("pack_buy"),
    command: z.strictObject({
      requestId: identifierSchema,
      recipeId: identifierSchema,
      expectedPolicyId: identifierSchema,
      quantity: packQuantitySchema,
      expectedRevision: amountSchema,
      expectedTotal: positiveAmountSchema,
    }),
  }),
  z.strictObject({
    kind: z.literal("pack_open"),
    command: z.strictObject({
      requestId: identifierSchema,
      inventoryAssetId: identifierSchema,
    }),
  }),
]);
export type EconomyCommand = z.infer<typeof economyCommandSchema>;

/** Committed contents only; server randomness and private recipe proofs never cross this boundary. */
export const ownedPackOutcomeSchema = z.strictObject({
  accountId: identifierSchema,
  outcome: z
    .strictObject({
      id: identifierSchema,
      inventoryAssetId: identifierSchema,
      recipeId: identifierSchema,
      operationId: identifierSchema,
      contents: z
        .array(
          z.strictObject({
            printingId: identifierSchema,
            quantity: positiveAmountSchema,
          }),
        )
        .min(1)
        .max(10000),
    })
    .refine((outcome) => {
      const ids = new Set<string>();
      let total = 0n;
      for (const card of outcome.contents) {
        if (
          !positiveAmountSchema.safeParse(card.quantity).success ||
          ids.has(card.printingId)
        )
          return false;
        ids.add(card.printingId);
        total += BigInt(card.quantity);
      }
      return total <= 64000n;
    }),
});
export type OwnedPackOutcome = z.infer<typeof ownedPackOutcomeSchema>;

/** Fixed-target commands cannot accept an acknowledgment for a different resource. */
export function economyAckMatches(
  command: EconomyCommand,
  resourceId: string,
): boolean {
  switch (command.kind) {
    case "trade_response":
      return resourceId === command.command.tradeId;
    case "listing_cancel":
    case "listing_buy":
      return resourceId === command.command.listingId;
    default:
      return identifierSchema.safeParse(resourceId).success;
  }
}
export const economyCommandErrorsSchema = z.union([
  z.strictObject({
    account: z.enum(["account unavailable", "market accounts unavailable"]),
  }),
  z.strictObject({ contact: z.literal("trade contact unavailable") }),
  z.strictObject({
    assets: z.enum([
      "invalid trade bundle",
      "trade assets unavailable",
      "not enough available offered assets",
      "trade assets no longer available",
      "not enough available assets",
      "market assets no longer available",
    ]),
  }),
  z.strictObject({
    trade: z.enum(["trade action unavailable", "trade is already terminal"]),
  }),
  z.strictObject({
    listing: z.enum([
      "invalid whole-lot listing",
      "listing unavailable",
      "listing terms or state changed",
    ]),
  }),
  z.strictObject({
    value: z.literal("restricted outgoing value exceeds the allowed limit"),
  }),
  z.strictObject({
    product: z.enum(["opening recipe not qualified", "product unavailable"]),
  }),
  z.strictObject({
    quote: z.enum([
      "invalid purchase terms",
      "stock or price changed; request a new quote",
    ]),
  }),
  z.strictObject({ coins: z.literal("not enough available coins") }),
  z.strictObject({
    pack: z.enum([
      "owned pack unavailable",
      "opening recipe not qualified",
      "no unheld pack available",
    ]),
  }),
]);
