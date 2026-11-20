/** Anonymous market depth and owner-only commitments; exact integers never use JavaScript Number. */
import { z } from "zod";
import {
  amountSchema,
  positiveAmountSchema,
  identifierSchema,
  instantSchema,
  instantNanoseconds,
} from "./primitives.ts";
import {
  predictionSideSchema as side,
  predictionBidSchema as bid,
  predictionConditionSchema,
  predictionTargetUsdSchema as price,
} from "./prediction-values.ts";
const reference = z
  .string()
  .min(1)
  .refine(
    (v) =>
      new TextEncoder().encode(v).length <= 128 &&
      Array.from(v).every((c) => {
        const n = c.charCodeAt(0);
        return n > 31 && (n < 127 || n > 159);
      }),
  );
export const predictionMarketSummarySchema = z
  .strictObject({
    id: identifierSchema,
    printingId: identifierSchema,
    priceMetric: reference,
    targetUsd: price,
    condition: predictionConditionSchema,
    deadline: instantSchema,
    tradingClosesAt: instantSchema,
    observationStartsAt: instantSchema,
    samplingPolicy: reference,
    terminalSlot: reference,
    rulesRevision: reference,
    payoutCoins: z.literal("100"),
    state: z.enum(["open", "closed", "resolved"]),
    acceptingOrders: z.boolean(),
    outcome: z.enum(["yes", "no", "void"]).nullable(),
    resolvedAt: instantSchema.nullable(),
  })
  .refine((v) => {
    const close = instantNanoseconds(v.tradingClosesAt),
      start = instantNanoseconds(v.observationStartsAt),
      end = instantNanoseconds(v.deadline);
    return (
      close !== null &&
      start !== null &&
      end !== null &&
      close < start &&
      start <= end &&
      (!v.acceptingOrders || v.state === "open") &&
      (v.state === "resolved"
        ? v.outcome !== null && v.resolvedAt !== null
        : v.outcome === null && v.resolvedAt === null)
    );
  });
/** At most100,000 orders below10^78 units sum below10^83; this is not an account amount. */
export const predictionDepthQuantitySchema = z
  .string()
  .regex(/^[1-9][0-9]{0,82}$/);
const depthLevel = z.strictObject({
  side,
  limitCost: bid,
  quantity: predictionDepthQuantitySchema,
});
export const predictionMarketDetailSchema = z
  .strictObject({
    terms: predictionMarketSummarySchema,
    book: z.array(depthLevel).max(198),
  })
  .refine((v) => {
    if (!v.terms.acceptingOrders && v.book.length) return false;
    if (!v.book.every((level) => bid.safeParse(level.limitCost).success))
      return false;
    return v.book.every((level, index) => {
      const previous = v.book[index - 1];
      if (!previous) return true;
      if (previous.side !== level.side) return previous.side === "no";
      return BigInt(previous.limitCost) > BigInt(level.limitCost);
    });
  });
export const ownedPredictionMarketSchema = z.strictObject({
  accountId: identifierSchema,
  market: predictionMarketDetailSchema,
});
export const predictionOrderSchema = z
  .strictObject({
    id: identifierSchema,
    marketId: identifierSchema,
    side,
    limitCost: bid,
    quantity: positiveAmountSchema,
    filledQuantity: amountSchema,
    actualFilledCost: amountSchema,
    unfilledReservation: amountSchema,
    state: z.enum(["open", "filled", "canceled", "closed", "settled"]),
    acceptedSequence: positiveAmountSchema,
    acceptedAt: instantSchema,
  })
  .refine((v) => {
    if (
      ![
        v.quantity,
        v.filledQuantity,
        v.actualFilledCost,
        v.unfilledReservation,
      ].every((n) => amountSchema.safeParse(n).success) ||
      !bid.safeParse(v.limitCost).success
    )
      return false;
    const qty = BigInt(v.quantity),
      filled = BigInt(v.filledQuantity),
      limit = BigInt(v.limitCost),
      cost = BigInt(v.actualFilledCost);
    return (
      qty > 0n &&
      filled <= qty &&
      qty * limit < 10n ** 78n &&
      cost >= filled &&
      cost <= filled * limit &&
      (v.state !== "filled" || filled === qty) &&
      (v.state !== "open" || filled < qty) &&
      BigInt(v.unfilledReservation) ===
        (v.state === "open" ? (qty - filled) * limit : 0n)
    );
  });
export const predictionPositionSchema = z
  .strictObject({
    id: identifierSchema,
    marketId: identifierSchema,
    orderId: identifierSchema,
    side,
    quantity: positiveAmountSchema,
    actualCost: positiveAmountSchema,
    fixedPayout: positiveAmountSchema,
    matchedAt: instantSchema,
  })
  .refine((v) => {
    if (
      ![v.quantity, v.actualCost, v.fixedPayout].every(
        (n) => positiveAmountSchema.safeParse(n).success,
      )
    )
      return false;
    const qty = BigInt(v.quantity),
      cost = BigInt(v.actualCost),
      payout = BigInt(v.fixedPayout);
    return (
      payout === 100n && cost >= qty && cost < qty * payout && cost % qty === 0n
    );
  });
function page<T extends z.ZodType<{ id: string }>>(item: T) {
  return z
    .strictObject({
      accountId: identifierSchema,
      items: z.array(item).max(100),
      nextCursor: identifierSchema.nullable(),
    })
    .refine(
      (v) =>
        v.items.every(
          (item, index) =>
            index === 0 || (v.items[index - 1]?.id ?? "") < item.id,
        ) &&
        (v.nextCursor === null || v.nextCursor === v.items.at(-1)?.id),
    );
}
export const predictionMarketPageSchema = page(predictionMarketSummarySchema);
export const predictionOrderPageSchema = page(predictionOrderSchema);
export const predictionPositionPageSchema = page(predictionPositionSchema);
export type OwnedPredictionMarket = z.infer<typeof ownedPredictionMarketSchema>;
export type PredictionMarketSummary = z.infer<
  typeof predictionMarketSummarySchema
>;
export type PredictionOrder = z.infer<typeof predictionOrderSchema>;
export type PredictionPosition = z.infer<typeof predictionPositionSchema>;
