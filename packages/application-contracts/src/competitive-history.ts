/** Exact stored credit/reward facts; reads never score or claim rewards. */
import { z } from "zod";
import {
  identifierSchema,
  amountSchema,
  instantSchema,
  daySchema,
} from "./primitives.ts";
import { validProgressionPage } from "./progression.ts";
export const ratingDeltaSchema = z
  .string()
  .regex(/^(0|[1-9][0-9]{0,77}|-[1-9][0-9]{0,77})$/, { abort: true });
export const competitiveCreditSchema = z
  .strictObject({
    id: identifierSchema,
    matchResultId: identifierSchema,
    formatId: identifierSchema,
    seasonId: identifierSchema.nullable(),
    win: z.boolean(),
    completedAt: instantSchema,
    ratingBefore: amountSchema.nullable(),
    ratingDelta: ratingDeltaSchema.nullable(),
    ratingAfter: amountSchema.nullable(),
  })
  .refine((r) => {
    if (r.seasonId === null)
      return (
        r.ratingBefore === null &&
        r.ratingDelta === null &&
        r.ratingAfter === null
      );
    if (
      r.ratingBefore === null ||
      r.ratingDelta === null ||
      r.ratingAfter === null
    )
      return false;
    const after = BigInt(r.ratingBefore) + BigInt(r.ratingDelta);
    return BigInt(r.ratingAfter) === (after < 0n ? 0n : after);
  });
export const creditPageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(competitiveCreditSchema),
  nextCursor: identifierSchema.nullable(),
});
export const dailyWinRecordSchema = z.strictObject({
  id: identifierSchema,
  utcDay: daySchema,
  wins: amountSchema,
  issuedCoins: amountSchema,
  revision: amountSchema,
});
export const dailyWinPageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(dailyWinRecordSchema),
  nextCursor: identifierSchema.nullable(),
});
export function validCreditPage(
  page: z.infer<typeof creditPageSchema>,
  owner: string,
  after: string | null,
  limit: number,
  formatId: string | null,
  seasonId: string | null,
): boolean {
  return (
    validProgressionPage(page, owner, after, limit) &&
    page.items.every(
      (r) =>
        (formatId === null || r.formatId === formatId) &&
        (seasonId === null || r.seasonId === seasonId),
    )
  );
}
export type CreditPage = z.infer<typeof creditPageSchema>;
export type DailyWinPage = z.infer<typeof dailyWinPageSchema>;
