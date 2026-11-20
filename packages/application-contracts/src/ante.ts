/** Human stakes and redacted accepted terms; private funding/hold/result proofs never enter this protocol. */
import { z } from "zod";
import {
  amountSchema,
  identifierSchema,
  instantSchema,
  positiveAmountSchema,
  daySchema,
} from "./primitives.ts";
import { assetBundleSchema } from "./economy.ts";
import { rationalSchema } from "./exchange-reads.ts";
import { makeCommandDispositionSchema } from "./command-protocol.ts";
const hashSchema = z.string().regex(/^[0-9a-f]{64}$/);
export const offerAnteSchema = z.strictObject({
  requestId: identifierSchema,
  expectedTermsRevision: amountSchema,
  assets: assetBundleSchema,
});
export const acceptAnteSchema = z.strictObject({
  requestId: identifierSchema,
  expectedTermsRevision: amountSchema,
  termsHash: hashSchema,
});
export const cancelAnteSchema = z.strictObject({
  requestId: identifierSchema,
  expectedTermsRevision: amountSchema,
});
export const anteOfferAckSchema = z.strictObject({
  resourceId: identifierSchema,
  termsRevision: amountSchema,
  termsHash: hashSchema,
});
export const anteAcceptAckSchema = z.strictObject({
  resourceId: identifierSchema,
  state: z.enum(["offered", "committed", "canceled"]),
  termsRevision: amountSchema,
});
export const anteCancelAckSchema = z.strictObject({
  resourceId: identifierSchema,
  state: z.literal("canceled"),
});
export const anteOfferErrorsSchema = z.union([
  z.strictObject({ match: z.literal("unavailable initial ante window") }),
  z.strictObject({ terms: z.literal("stale or no longer offered") }),
]);
export const anteAcceptErrorsSchema = z.union([
  z.strictObject({
    terms: z.enum([
      "unavailable or stale ante agreement",
      "ante asset sources changed",
    ]),
  }),
  z.strictObject({
    stakes: z.enum([
      "a positive stake cannot stand alone",
      "restricted outgoing stake exceeds130percent of the largest other stake",
    ]),
  }),
  z.strictObject({ assets: z.literal("ante reservation unavailable") }),
  z.strictObject({
    allowance: z.literal("AI exposure differs from accepted start-day stakes"),
  }),
]);
export const anteCancelErrorsSchema = z.strictObject({
  terms: z.literal("ante cannot be canceled at this basis"),
});
export const anteOfferDispositionSchema = makeCommandDispositionSchema(
  anteOfferErrorsSchema,
  anteOfferAckSchema,
);
export const anteAcceptDispositionSchema = makeCommandDispositionSchema(
  anteAcceptErrorsSchema,
  anteAcceptAckSchema,
);
export const anteCancelDispositionSchema = makeCommandDispositionSchema(
  anteCancelErrorsSchema,
  anteCancelAckSchema,
);
export function validAnteOfferAck(
  ack: z.infer<typeof anteOfferAckSchema>,
  input: z.infer<typeof offerAnteSchema>,
): boolean {
  return BigInt(ack.termsRevision) === BigInt(input.expectedTermsRevision) + 1n;
}
export function validAnteAcceptAck(
  ack: z.infer<typeof anteAcceptAckSchema>,
  commitment: string,
  input: z.infer<typeof acceptAnteSchema>,
): boolean {
  return (
    ack.resourceId === commitment &&
    ack.termsRevision === input.expectedTermsRevision
  );
}
export function validAnteCancelAck(
  ack: z.infer<typeof anteCancelAckSchema>,
  commitment: string,
): boolean {
  return ack.resourceId === commitment;
}
export const displayStakeSchema = z
  .strictObject({
    seatId: identifierSchema,
    fundingKind: z.enum(["owned_assets", "ai_loss_allowance"]),
    aiCoins: amountSchema.nullable(),
    entries: assetBundleSchema,
    value: rationalSchema.refine((v) => v.numerator !== "0").nullable(),
  })
  .refine(
    (s) =>
      s.entries.every(
        (l, i) => i === 0 || l.assetId > (s.entries[i - 1]?.assetId ?? ""),
      ) &&
      (s.fundingKind === "owned_assets"
        ? s.aiCoins === null && s.entries.length > 0
        : s.entries.length === 0 &&
          s.aiCoins !== null &&
          BigInt(s.aiCoins) > 0n &&
          BigInt(s.aiCoins) <= 300n),
  );
export const displayAllocationSchema = z.strictObject({
  sourceSeatId: identifierSchema,
  assetId: identifierSchema,
  recipientSeatId: identifierSchema,
  quantity: positiveAmountSchema,
  classification: z.enum([
    "return",
    "transfer",
    "retire",
    "ai_unissued",
    "ai_issue",
  ]),
});
export const anteAgreementSchema = z
  .strictObject({
    id: identifierSchema,
    state: z.enum(["offered", "committed", "settled", "refunded", "canceled"]),
    termsRevision: amountSchema,
    termsHash: hashSchema.nullable(),
    acceptedSeatIds: z
      .array(identifierSchema)
      .max(6)
      .refine((a) => new Set(a).size === a.length),
    acceptedAt: instantSchema.nullable(),
    startDay: daySchema.nullable(),
    stakes: z.array(displayStakeSchema).max(6),
    settlementResultId: identifierSchema.nullable(),
    allocations: z.array(displayAllocationSchema).max(1200).nullable(),
  })
  .refine(
    (a) =>
      a.stakes.every(
        (s, i) => i === 0 || s.seatId > (a.stakes[i - 1]?.seatId ?? ""),
      ) &&
      (a.allocations !== null) === (a.settlementResultId !== null) &&
      (a.state === "settled" || a.state === "refunded") ===
        (a.allocations !== null) &&
      (a.allocations === null ||
        a.allocations.every((row) =>
          a.stakes.some(
            (s) =>
              s.seatId === row.sourceSeatId &&
              (s.fundingKind === "ai_loss_allowance" ||
                s.entries.some((l) => l.assetId === row.assetId)),
          ),
        )),
  );
export const anteDetailSchema = z.strictObject({
  accountId: identifierSchema,
  matchId: identifierSchema,
  ownSeatId: identifierSchema,
  agreement: anteAgreementSchema.nullable(),
});
export function validAnteDetail(
  detail: z.infer<typeof anteDetailSchema>,
  owner: string,
  matchId: string,
): boolean {
  return (
    identifierSchema.safeParse(owner).success &&
    identifierSchema.safeParse(matchId).success &&
    detail.accountId === owner &&
    detail.matchId === matchId
  );
}
export type AnteDetail = z.infer<typeof anteDetailSchema>;
export type OfferAnte = z.infer<typeof offerAnteSchema>;
export type AcceptAnte = z.infer<typeof acceptAnteSchema>;
export type CancelAnte = z.infer<typeof cancelAnteSchema>;
