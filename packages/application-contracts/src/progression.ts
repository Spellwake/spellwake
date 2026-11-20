/** Stored owner progression protocol paired with Rust progression DTOs. */
import { z } from "zod";
import {
  daySchema,
  identifierSchema,
  amountSchema,
  positiveAmountSchema,
  instantSchema,
  instantNanoseconds,
} from "./primitives.ts";

export const questObjectiveSchema = z.strictObject({
  definitionKey: z.string(),
  revision: z.string(),
  objectiveRef: z.string(),
  target: positiveAmountSchema,
  rewardCoins: z.enum(["500", "750"]),
});
export const dailyQuestAllowanceSchema = z.strictObject({
  generatedAssignmentId: identifierSchema.nullable(),
  rerollsUsed: z.enum(["0", "1"]),
  revision: amountSchema,
});
export const questAssignmentSchema = z
  .strictObject({
    id: identifierSchema,
    definitionId: identifierSchema,
    sourceDay: daySchema,
    assignedAt: instantSchema,
    progress: amountSchema,
    completedAt: instantSchema.nullable(),
    replacedById: identifierSchema.nullable(),
    objective: questObjectiveSchema.nullable(),
  })
  .refine((item) => {
    if (
      item.replacedById === item.id ||
      (item.replacedById !== null && item.completedAt !== null)
    )
      return false;
    if (item.completedAt !== null) {
      const completed = instantNanoseconds(item.completedAt);
      const assigned = instantNanoseconds(item.assignedAt);
      if (completed === null || assigned === null || completed < assigned)
        return false;
    }
    return (
      item.objective === null ||
      (BigInt(item.progress) <= BigInt(item.objective.target) &&
        (item.completedAt !== null) ===
          (item.progress === item.objective.target))
    );
  });
export const questPageSchema = z.strictObject({
  accountId: identifierSchema,
  utcDay: daySchema,
  daily: dailyQuestAllowanceSchema.nullable(),
  items: z.array(questAssignmentSchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export const rankSummarySchema = z.strictObject({
  id: identifierSchema,
  formatId: identifierSchema,
  seasonId: identifierSchema,
  tier: z.string(),
  rating: amountSchema,
  revision: amountSchema,
  finalized: z.boolean(),
});
export const rankPageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(rankSummarySchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export const seasonSummarySchema = z
  .strictObject({
    id: identifierSchema,
    year: z.string(),
    quarter: z.string(),
    startAt: instantSchema,
    endAt: instantSchema,
    state: z.string(),
  })
  .refine((item) => {
    const start = new Date(item.startAt);
    const end = new Date(item.endAt);
    const month = start.getUTCMonth();
    if (
      item.year !== String(start.getUTCFullYear()) ||
      item.quarter !== String(Math.floor(month / 3) + 1) ||
      ![0, 3, 6, 9].includes(month) ||
      start.getUTCDate() !== 1 ||
      start.getUTCHours() !== 0 ||
      start.getUTCMinutes() !== 0 ||
      start.getUTCSeconds() !== 0 ||
      start.getUTCMilliseconds() !== 0
    )
      return false;
    const expected = new Date(start);
    expected.setUTCMonth(month + 3);
    return (
      expected.getTime() === end.getTime() &&
      instantNanoseconds(item.startAt) ===
        BigInt(start.getTime()) * 1_000_000n &&
      instantNanoseconds(item.endAt) === BigInt(end.getTime()) * 1_000_000n
    );
  });
export const seasonPageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(seasonSummarySchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export type QuestPage = z.infer<typeof questPageSchema>;
export type RankPage = z.infer<typeof rankPageSchema>;
export type SeasonPage = z.infer<typeof seasonPageSchema>;
/** Page validation includes original owner/filter/cursor context, not only JSON shape. */
export function validProgressionPage(
  page: {
    accountId: string;
    items: ReadonlyArray<{ id: string }>;
    nextCursor: string | null;
  },
  owner: string,
  after: string | null,
  limit: number,
): boolean {
  return (
    identifierSchema.safeParse(owner).success &&
    page.accountId === owner &&
    Number.isInteger(limit) &&
    limit >= 1 &&
    limit <= 100 &&
    page.items.length <= limit &&
    page.items.every((item) => after === null || item.id > after) &&
    page.items.every(
      (item, index) =>
        index === 0 || item.id > (page.items[index - 1]?.id ?? ""),
    ) &&
    page.nextCursor ===
      (page.items.length === limit ? (page.items.at(-1)?.id ?? null) : null)
  );
}

/** Bind the original rank filters as well as common owner/page context. */
export function validRankPage(
  page: RankPage,
  owner: string,
  after: string | null,
  limit: number,
  formatId: string | null,
  seasonId: string | null,
): boolean {
  return (
    validProgressionPage(page, owner, after, limit) &&
    page.items.every(
      (item) =>
        (formatId === null || item.formatId === formatId) &&
        (seasonId === null || item.seasonId === seasonId),
    )
  );
}
