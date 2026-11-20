/** Frozen ballot pages and original ordered approval commands paired with Rust supply_ballot. */
import { z } from "zod";
import {
  identifierSchema,
  amountSchema,
  instantSchema,
  instantNanoseconds,
} from "./primitives.ts";
import { makeCommandDispositionSchema } from "./command-protocol.ts";
import { validProgressionPage } from "./progression.ts";
export const replaceBallotSchema = z.strictObject({
  requestId: identifierSchema,
  setIds: z
    .array(identifierSchema)
    .max(10)
    .refine((ids) => new Set(ids).size === ids.length),
});
export const ballotAckSchema = z.strictObject({
  resourceId: identifierSchema,
  approvals: amountSchema,
});
export const ballotErrorsSchema = z.union([
  z.strictObject({
    account: z.enum(["unavailable", "qualifying real game required"]),
  }),
  z.strictObject({ ballot: z.literal("closed or unavailable") }),
  z.strictObject({ sets: z.literal("outside frozen candidates") }),
]);
export const ballotDispositionSchema = makeCommandDispositionSchema(
  ballotErrorsSchema,
  ballotAckSchema,
);
export function validBallotAck(
  ack: z.infer<typeof ballotAckSchema>,
  seasonId: string,
  input: z.infer<typeof replaceBallotSchema>,
): boolean {
  return (
    ack.resourceId === seasonId && ack.approvals === String(input.setIds.length)
  );
}
const populationMonthSchema = z.strictObject({
  month: z
    .string()
    .regex(/^\d{4}-\d{2}$/)
    .refine((s) => {
      const d = new Date(`${s}-01T00:00:00Z`);
      return (
        Number.isFinite(d.getTime()) && d.toISOString().startsWith(`${s}-01`)
      );
    }),
  accounts: amountSchema,
});
export const ballotPublicationSchema = z
  .strictObject({
    population: z.array(populationMonthSchema).length(3),
    populationSum: amountSchema,
    coefficient: amountSchema.refine((s) => s !== "0" && s.length <= 9),
    quantity: amountSchema,
  })
  .refine((p) => {
    const sum = p.population.reduce((n, r) => n + BigInt(r.accounts), 0n);
    return (
      sum === BigInt(p.populationSum) &&
      p.population.every((r, i) => {
        if (i === 0) return true;
        const previous = p.population[i - 1];
        if (previous === undefined) return false;
        const date = new Date(`${previous.month}-01T00:00:00Z`);
        date.setUTCMonth(date.getUTCMonth() + 1);
        return date.toISOString().startsWith(`${r.month}-01`);
      })
    );
  });
export const ballotCandidateSchema = z.strictObject({
  setId: identifierSchema,
  name: z.string(),
  approved: z.boolean(),
  finalApprovals: amountSchema.nullable(),
  winner: z.boolean().nullable(),
  dropQuantity: amountSchema.nullable(),
});
export const ballotPageSchema = z
  .strictObject({
    accountId: identifierSchema,
    seasonId: identifierSchema,
    cutoff: instantSchema,
    openedAt: instantSchema,
    isOpen: z.boolean(),
    canVote: z.boolean(),
    qualifies: z.boolean(),
    approvedSetIds: z.array(identifierSchema).max(10),
    publication: ballotPublicationSchema.nullable(),
    items: z.array(ballotCandidateSchema),
    nextCursor: identifierSchema.nullable(),
  })
  .refine((p) => {
    const open = instantNanoseconds(p.openedAt),
      cutoff = instantNanoseconds(p.cutoff);
    return (
      open !== null &&
      cutoff !== null &&
      open < cutoff &&
      (!p.canVote || (p.isOpen && p.qualifies)) &&
      p.approvedSetIds.every(
        (id, i) => i === 0 || id > (p.approvedSetIds[i - 1] ?? ""),
      ) &&
      (p.publication === null || !p.isOpen) &&
      p.items.every(
        (c) =>
          c.approved === p.approvedSetIds.includes(c.setId) &&
          (p.publication === null
            ? c.finalApprovals === null &&
              c.winner === null &&
              c.dropQuantity === null
            : c.finalApprovals !== null &&
              c.winner !== null &&
              c.winner === (c.dropQuantity !== null)),
      )
    );
  });
export function validBallotPage(
  page: z.infer<typeof ballotPageSchema>,
  owner: string,
  seasonId: string,
  after: string | null,
  limit: number,
): boolean {
  return (
    identifierSchema.safeParse(seasonId).success &&
    page.seasonId === seasonId &&
    validProgressionPage(
      { ...page, items: page.items.map((row) => ({ id: row.setId })) },
      owner,
      after,
      limit,
    )
  );
}
export type BallotPage = z.infer<typeof ballotPageSchema>;
export type ReplaceBallot = z.infer<typeof replaceBallotSchema>;
