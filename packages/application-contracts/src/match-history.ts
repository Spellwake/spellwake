/** Participant scalar metadata paired with Rust match_history; no native view/replay content. */
import { z } from "zod";
import {
  amountSchema,
  identifierSchema,
  instantSchema,
  instantNanoseconds,
  publicAccountIdentitySchema,
} from "./primitives.ts";
import { validProgressionPage } from "./progression.ts";
const outcomeSchema = z.enum(["win", "draw", "no_contest"]);
const winnersSchema = z
  .array(identifierSchema)
  .max(6)
  .refine((ids) => new Set(ids).size === ids.length)
  .nullable();
const resultWinners = (r: {
  outcome: z.infer<typeof outcomeSchema>;
  winnerSeatIds: string[] | null;
}): boolean =>
  r.winnerSeatIds === null ||
  (r.outcome === "win") === r.winnerSeatIds.length > 0;
// PostgreSQL btrim removes ASCII space only; retain the original source text.
const text = z.string().refine((s) => {
  const n = Array.from(s.replace(/^ +| +$/g, "")).length;
  return n >= 1 && n <= 2048 && !s.includes("\0");
});
const nonblank = text;
export const matchSummarySchema = z
  .strictObject({
    id: identifierSchema,
    formatId: identifierSchema,
    profileId: identifierSchema,
    mode: z.enum(["open", "collection"]),
    competitive: z.boolean(),
    structure: z.enum(["bo1", "bo3"]),
    state: z.enum([
      "admitting",
      "ready",
      "active",
      "between_games",
      "completed",
      "no_contest",
    ]),
    revision: amountSchema,
    startedAt: instantSchema,
    currentGameId: identifierSchema.nullable(),
    reservedGameId: identifierSchema.nullable(),
    availability: z.enum(["available", "recovering", "unrecoverable"]),
    availabilityRevision: amountSchema,
  })
  .refine(
    (r) =>
      (!r.competitive || r.mode === "collection") &&
      !(r.currentGameId !== null && r.reservedGameId !== null),
  );
export const matchResultSchema = z
  .strictObject({
    id: identifierSchema,
    outcome: outcomeSchema,
    committedAt: instantSchema,
    rankingFormatId: identifierSchema.nullable(),
    rankSequence: amountSchema.nullable(),
    winnerSeatIds: winnersSchema,
    terminalKind: z.enum([
      "game_outcome",
      "unrecoverable_incident",
      "bank_forfeit",
    ]),
  })
  .refine(
    (r) =>
      resultWinners(r) &&
      (r.rankingFormatId !== null) === (r.rankSequence !== null),
  );
export const matchDetailSchema = z
  .strictObject({
    accountId: identifierSchema,
    match: matchSummarySchema,
    ownSeatId: identifierSchema,
    result: matchResultSchema.nullable(),
  })
  .refine((r) => {
    if (r.result === null) return true;
    const start = instantNanoseconds(r.match.startedAt),
      end = instantNanoseconds(r.result.committedAt);
    return start !== null && end !== null && end >= start;
  });
export const gameResultSchema = z
  .strictObject({
    id: identifierSchema,
    outcome: outcomeSchema,
    winnerSeatIds: winnersSchema,
    endReason: text,
  })
  .refine(resultWinners);
export const gameSummarySchema = z.strictObject({
  id: identifierSchema,
  // Public locator only; explicit null before context creation, never optional.
  contextId: identifierSchema.nullable(),
  gameIndex: amountSchema,
  state: z.enum(["reserved", "active", "completed", "no_contest", "canceled"]),
  acceptedRevision: amountSchema,
  result: gameResultSchema.nullable(),
});
export const clockSummarySchema = z.strictObject({
  remainingMs: amountSchema,
  revision: amountSchema,
  policyRef: nonblank,
  paused: z.boolean(),
  lastAccountedAt: instantSchema,
});
export const matchSeatSchema = z
  .strictObject({
    id: identifierSchema,
    seatIndex: amountSchema.refine((s) => BigInt(s) < 6n),
    controllerKind: z.enum(["human", "ai"]),
    person: publicAccountIdentitySchema.nullable(),
    clock: clockSummarySchema.nullable(),
  })
  .refine((s) => (s.controllerKind === "human") === (s.person !== null));
export const ownDecisionSchema = z
  .strictObject({
    id: identifierSchema,
    seatId: identifierSchema,
    decisionKind: nonblank,
    basisRevision: amountSchema,
    deadlineAt: instantSchema.nullable(),
    clockRevision: amountSchema,
    state: z.enum(["pending", "answered", "canceled"]),
    afterGameId: identifierSchema.nullable(),
    selectedDeckRevisionId: identifierSchema.nullable(),
    openedAt: instantSchema,
    deadlinePolicyRef: nonblank.nullable(),
    deadlineRevision: amountSchema,
    deadlineAvailabilityRevision: amountSchema.nullable(),
    deadlineRemainingUs: amountSchema.nullable(),
    deadlineAccountedAt: instantSchema.nullable(),
  })
  .refine((d) =>
    d.deadlinePolicyRef === null
      ? d.deadlineRevision === "0" &&
        d.deadlineAt === null &&
        d.deadlineAvailabilityRevision === null &&
        d.deadlineRemainingUs === null &&
        d.deadlineAccountedAt === null
      : d.deadlineRevision !== "0" &&
        d.deadlineAvailabilityRevision !== null &&
        d.deadlineAccountedAt !== null &&
        (d.deadlineAt !== null) !== (d.deadlineRemainingUs !== null),
  );
export const matchListSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(matchSummarySchema),
  nextCursor: identifierSchema.nullable(),
});
export const gamePageSchema = z.strictObject({
  accountId: identifierSchema,
  matchId: identifierSchema,
  items: z.array(gameSummarySchema).refine((items) => {
    const contexts = items.flatMap((item) =>
      item.contextId === null ? [] : [item.contextId],
    );
    return new Set(contexts).size === contexts.length;
  }),
  nextCursor: identifierSchema.nullable(),
});
export const seatPageSchema = z.strictObject({
  accountId: identifierSchema,
  matchId: identifierSchema,
  items: z.array(matchSeatSchema),
  nextCursor: identifierSchema.nullable(),
});
export const decisionPageSchema = z.strictObject({
  accountId: identifierSchema,
  matchId: identifierSchema,
  items: z.array(ownDecisionSchema),
  nextCursor: identifierSchema.nullable(),
});
/** Linked objects may be off-page; context validation does not invent complete-roster semantics. */
export function validMatchPage(
  page:
    | z.infer<typeof gamePageSchema>
    | z.infer<typeof seatPageSchema>
    | z.infer<typeof decisionPageSchema>,
  owner: string,
  matchId: string,
  after: string | null,
  limit: number,
): boolean {
  return (
    page.matchId === matchId &&
    identifierSchema.safeParse(matchId).success &&
    validProgressionPage(page, owner, after, limit)
  );
}
export function validMatchDetail(
  detail: z.infer<typeof matchDetailSchema>,
  owner: string,
  matchId: string,
): boolean {
  return (
    identifierSchema.safeParse(owner).success &&
    identifierSchema.safeParse(matchId).success &&
    detail.accountId === owner &&
    detail.match.id === matchId
  );
}
export type MatchDetail = z.infer<typeof matchDetailSchema>;
export type MatchList = z.infer<typeof matchListSchema>;
export type GamePage = z.infer<typeof gamePageSchema>;
export type SeatPage = z.infer<typeof seatPageSchema>;
export type DecisionPage = z.infer<typeof decisionPageSchema>;
