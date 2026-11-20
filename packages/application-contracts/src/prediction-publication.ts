/** Publication and safe preflight; evidence qualification/current time remain server authority. */
import { z } from "zod";
import {
  identifierSchema,
  instantSchema,
  instantNanoseconds,
} from "./primitives.ts";
import {
  predictionConditionSchema,
  predictionUsdDecimalSchema,
  predictionTargetUsdSchema,
} from "./prediction-values.ts";
const publicationInstant = instantSchema.refine((value) => {
  const date = new Date(value),
    fraction = /\.(\d+)/.exec(value)?.[1] ?? "";
  return (
    /^[0-9]{4}-/.test(value) &&
    date.getUTCFullYear() >= 1 &&
    date.getUTCFullYear() <= 9999 &&
    fraction.length <= 6
  );
});
export const publishPredictionSchema = z
  .strictObject({
    requestId: identifierSchema,
    printingId: identifierSchema,
    probeId: identifierSchema,
    targetUsd: predictionTargetUsdSchema,
    condition: predictionConditionSchema,
    tradingClosesAt: publicationInstant,
    observationStartsAt: publicationInstant,
    deadline: publicationInstant,
  })
  .refine((value) => {
    const close = instantNanoseconds(value.tradingClosesAt),
      start = instantNanoseconds(value.observationStartsAt),
      end = instantNanoseconds(value.deadline);
    return (
      close !== null &&
      start !== null &&
      end !== null &&
      close < start &&
      start <= end &&
      end - start <= 366n * 86400n * 1_000_000_000n
    );
  });
export const predictionPublicationSourceSchema = z
  .strictObject({
    printingId: identifierSchema,
    probeId: identifierSchema,
    priceMetric: z.enum([
      "scryfall:usd",
      "scryfall:usd_foil",
      "scryfall:usd_etched",
    ]),
    currentUsd: predictionUsdDecimalSchema,
    acquiredAt: instantSchema,
    usableUntil: instantSchema,
  })
  .refine((value) => {
    const acquired = instantNanoseconds(value.acquiredAt),
      until = instantNanoseconds(value.usableUntil);
    return (
      acquired !== null &&
      until !== null &&
      until - acquired === 24n * 3600n * 1_000_000_000n
    );
  });
export const ownedPredictionPublicationSourceSchema = z.strictObject({
  accountId: identifierSchema,
  source: predictionPublicationSourceSchema,
});
export type PublishPrediction = z.infer<typeof publishPredictionSchema>;
export type OwnedPredictionPublicationSource = z.infer<
  typeof ownedPredictionPublicationSourceSchema
>;

/** A preparation job is shared by exact source printing/day; completion is not finish-price eligibility. */
export const predictionSourcePreparationSchema = z.strictObject({
  accountId: identifierSchema,
  printingId: identifierSchema,
  slot: z.iso
    .date()
    .refine((value) => /^[0-9]{4}-/.test(value) && !value.startsWith("0000-")),
  state: z.enum(["requested", "completed", "unavailable"]),
});
export type PredictionSourcePreparation = z.infer<
  typeof predictionSourcePreparationSchema
>;
