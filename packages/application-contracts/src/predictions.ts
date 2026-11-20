/** Coin-only fixed-payout contract commitments; settlement and identity stay server authoritative. */
import { z } from "zod";
import { publishPredictionSchema } from "./prediction-publication.ts";
import {
  predictionBidSchema as bidSchema,
  predictionSideSchema,
} from "./prediction-values.ts";
import { identifierSchema, positiveAmountSchema } from "./primitives.ts";
export const placePredictionOrderSchema = z
  .strictObject({
    requestId: identifierSchema,
    marketId: identifierSchema,
    side: predictionSideSchema,
    limitCost: bidSchema,
    quantity: positiveAmountSchema,
  })
  .refine((value) => {
    if (
      !bidSchema.safeParse(value.limitCost).success ||
      !positiveAmountSchema.safeParse(value.quantity).success
    )
      return false;
    return BigInt(value.limitCost) * BigInt(value.quantity) < 10n ** 78n;
  });
export const cancelPredictionOrderSchema = z.strictObject({
  requestId: identifierSchema,
  orderId: identifierSchema,
});
export const predictionCommandSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("publish_market"),
    command: publishPredictionSchema,
  }),
  z.strictObject({
    kind: z.literal("place_order"),
    command: placePredictionOrderSchema,
  }),
  z.strictObject({
    kind: z.literal("cancel_order"),
    command: cancelPredictionOrderSchema,
  }),
]);
export type PredictionCommand = z.infer<typeof predictionCommandSchema>;
/** Only cancellation has an already-known output identity; placement returns its new order. */
export function predictionAckMatches(
  input: PredictionCommand,
  resourceId: string,
): boolean {
  return (
    identifierSchema.safeParse(resourceId).success &&
    (input.kind !== "cancel_order" || input.command.orderId === resourceId)
  );
}
export const predictionCommandErrorsSchema = z.union([
  z.strictObject({
    terms: z.enum([
      "invalid future sampled-price terms",
      "at least one complete daily acceptance window required",
    ]),
  }),
  z.strictObject({ printing: z.literal("printing unavailable") }),
  z.strictObject({
    price: z.enum([
      "qualified printing price unavailable",
      "usable current exact-printing price required",
    ]),
  }),
  z.strictObject({ account: z.literal("account unavailable") }),
  z.strictObject({
    order: z.enum([
      "invalid whole-contract bid",
      "prediction order unavailable",
      "no unfilled units to cancel",
    ]),
  }),
  z.strictObject({
    market: z.enum([
      "prediction market unavailable",
      "prediction trading is closed",
      "prediction order capacity reached",
    ]),
  }),
  z.strictObject({
    quantity: z.literal("reservation exceeds supported precision"),
  }),
  z.strictObject({ coins: z.literal("not enough available coins") }),
]);
