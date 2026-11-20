/** Caller projections omit private holds and per-line source evidence. */
import { z } from "zod";
import { assetIdentitySchema } from "./assets.ts";
import {
  amountSchema,
  identifierSchema,
  instantSchema,
  positiveAmountSchema,
  publicAccountIdentitySchema,
} from "./primitives.ts";

export const EXCHANGE_PAGE_LIMIT = 8;
const component = z.string().regex(/^(0|[1-9][0-9]{0,8191})$/, { abort: true });
const denominator = z.string().regex(/^[1-9][0-9]{0,8191}$/, { abort: true });
type Fraction = { numerator: string; denominator: string };
function parts(value: Fraction): [bigint, bigint] | null {
  if (
    !component.safeParse(value.numerator).success ||
    !denominator.safeParse(value.denominator).success
  )
    return null;
  return [BigInt(value.numerator), BigInt(value.denominator)];
}
export const rationalSchema = z
  .strictObject({ numerator: component, denominator })
  .refine((value) => {
    const parsed = parts(value);
    if (!parsed) return false;
    let [left, right] = parsed;
    while (right !== 0n) [left, right] = [right, left % right];
    return left === 1n;
  });
const valuationTotalSchema = z.strictObject({
  policyId: identifierSchema,
  catalogueRevision: amountSchema,
  total: rationalSchema,
});
export const settlementSummarySchema = z
  .strictObject({
    outgoing: valuationTotalSchema,
    incoming: valuationTotalSchema,
    outgoingAllowed: z.boolean(),
    incomingAllowed: z.boolean(),
    senderRestricted: z.boolean(),
    recipientRestricted: z.boolean(),
  })
  .refine((value) => {
    const a = parts(value.outgoing.total);
    const b = parts(value.incoming.total);
    if (!a || !b) return false;
    const allowed = 10n * a[0] * b[1] <= 13n * b[0] * a[1];
    const inverse = 10n * b[0] * a[1] <= 13n * a[0] * b[1];
    return (
      value.outgoing.policyId === value.incoming.policyId &&
      value.outgoing.catalogueRevision === value.incoming.catalogueRevision &&
      value.outgoingAllowed === allowed &&
      value.incomingAllowed === inverse &&
      (!value.senderRestricted || allowed) &&
      (!value.recipientRestricted || inverse)
    );
  });
export const tradeStateSchema = z.enum([
  "offered",
  "accepted",
  "rejected",
  "canceled",
]);
const tradeAssetSchema = z.strictObject({
  assetId: identifierSchema,
  quantity: positiveAmountSchema,
  asset: assetIdentitySchema,
});
const tradeBundleSchema = z
  .array(tradeAssetSchema)
  .max(100)
  .refine((items) =>
    items.every(
      (item, index) =>
        index === 0 || (items[index - 1]?.assetId ?? "") < item.assetId,
    ),
  );
export const tradeRecordSchema = z
  .strictObject({
    id: identifierSchema,
    sender: publicAccountIdentitySchema,
    recipient: publicAccountIdentitySchema,
    revision: z.literal("1"),
    state: tradeStateSchema,
    createdAt: instantSchema,
    give: tradeBundleSchema,
    request: tradeBundleSchema,
    settlement: settlementSummarySchema.nullable(),
  })
  .refine(
    (value) =>
      value.sender.id !== value.recipient.id &&
      value.give.length + value.request.length > 0 &&
      (value.state === "accepted") === (value.settlement !== null) &&
      (value.settlement === null ||
        ((value.settlement.outgoing.total.numerator === "0") ===
          (value.give.length === 0) &&
          (value.settlement.incoming.total.numerator === "0") ===
            (value.request.length === 0))),
  );
type TradeRecord = z.infer<typeof tradeRecordSchema>;
function tradeBelongsTo(value: TradeRecord, owner: string): boolean {
  return (
    (value.sender.id === owner && !value.sender.isDeleted) ||
    (value.recipient.id === owner && !value.recipient.isDeleted)
  );
}
const participantSchema = z.strictObject({
  buyer: publicAccountIdentitySchema.nullable(),
  paidCoins: positiveAmountSchema.nullable(),
  completedAt: instantSchema.nullable(),
  settlement: settlementSummarySchema.nullable(),
});
export const listingRecordSchema = z
  .strictObject({
    id: identifierSchema,
    seller: publicAccountIdentitySchema,
    assetId: identifierSchema,
    asset: assetIdentitySchema,
    quantity: positiveAmountSchema,
    totalPrice: positiveAmountSchema,
    state: z.enum(["listed", "sold", "canceled"]),
    revision: positiveAmountSchema,
    participant: participantSchema.nullable(),
  })
  .refine((value) => {
    if (value.asset.kind === "coin") return false;
    const p = value.participant;
    if (p === null) return value.state === "listed";
    if (p.buyer?.id === value.seller.id) return false;
    if (value.state !== "sold")
      return (
        p.buyer === null &&
        p.paidCoins === null &&
        p.completedAt === null &&
        p.settlement === null
      );
    if (
      p.buyer === null ||
      p.paidCoins !== value.totalPrice ||
      p.completedAt === null ||
      p.settlement === null
    )
      return false;
    const incoming = parts(p.settlement.incoming.total);
    return (
      incoming !== null &&
      positiveAmountSchema.safeParse(value.totalPrice).success &&
      incoming[0] === BigInt(value.totalPrice) * incoming[1] &&
      p.settlement.outgoing.total.numerator !== "0"
    );
  });
type ListingRecord = z.infer<typeof listingRecordSchema>;
function listingBelongsTo(value: ListingRecord, owner: string): boolean {
  if (value.seller.id === owner && value.seller.isDeleted) return false;
  const p = value.participant;
  if (p === null) return value.seller.id !== owner && value.state === "listed";
  return (
    value.seller.id === owner || (p.buyer?.id === owner && !p.buyer.isDeleted)
  );
}
export const ownedTradeSchema = z
  .strictObject({ accountId: identifierSchema, trade: tradeRecordSchema })
  .refine((value) => tradeBelongsTo(value.trade, value.accountId));
export const ownedListingSchema = z
  .strictObject({ accountId: identifierSchema, listing: listingRecordSchema })
  .refine((value) => listingBelongsTo(value.listing, value.accountId));
function orderedPage(value: {
  items: { id: string }[];
  nextCursor: string | null;
}): boolean {
  return (
    value.items.every(
      (item, index) =>
        index === 0 || (value.items[index - 1]?.id ?? "") < item.id,
    ) &&
    (value.nextCursor === null || value.items.at(-1)?.id === value.nextCursor)
  );
}
export const tradePageSchema = z
  .strictObject({
    accountId: identifierSchema,
    items: z.array(tradeRecordSchema).max(EXCHANGE_PAGE_LIMIT),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (value) =>
      orderedPage(value) &&
      value.items.every((item) => tradeBelongsTo(item, value.accountId)),
  );
export const listingPageSchema = z
  .strictObject({
    accountId: identifierSchema,
    items: z.array(listingRecordSchema).max(EXCHANGE_PAGE_LIMIT),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (value) =>
      orderedPage(value) &&
      value.items.every((item) => listingBelongsTo(item, value.accountId)),
  );
const pageQuery = {
  after: identifierSchema.nullable().optional(),
  limit: z.number().int().min(1).max(EXCHANGE_PAGE_LIMIT).optional(),
};
export const tradePageQuerySchema = z.strictObject({
  ...pageQuery,
  state: tradeStateSchema.optional(),
});
export const marketPageQuerySchema = z.strictObject({
  ...pageQuery,
  owned: z.boolean().optional(),
});
export type TradePageQuery = z.infer<typeof tradePageQuerySchema>;
export type MarketPageQuery = z.infer<typeof marketPageQuerySchema>;
export type OwnedTrade = z.infer<typeof ownedTradeSchema>;
export type OwnedListing = z.infer<typeof ownedListingSchema>;
