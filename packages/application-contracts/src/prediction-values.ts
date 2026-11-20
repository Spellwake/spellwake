/** Shared exact prediction values; these do not price collection trades. */
import { z } from "zod";
export const predictionSideSchema = z.enum(["yes", "no"]);
export const predictionBidSchema = z.string().regex(/^[1-9][0-9]?$/);
export const predictionConditionSchema = z.enum(["reached_by", "at_deadline"]);
export const predictionUsdDecimalSchema = z
  .string()
  .max(80)
  .regex(/^(0|[1-9][0-9]*)(\.[0-9]+)?$/);
export const predictionTargetUsdSchema = predictionUsdDecimalSchema.refine(
  (v) => /[1-9]/.test(v),
);
