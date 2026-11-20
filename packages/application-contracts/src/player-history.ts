/** Typed visible observations, never an executable native journal or arbitrary payload. */
import { z } from "zod";
import { amountSchema, identifierSchema } from "./primitives.ts";
import {
  playerBasisSchema,
  playerHandleSchema,
  rulesIntegerSchema,
  samePlayerWireValue,
  playerBasisRegresses,
} from "./player-primitives.ts";
import { samePlayerBasis, playerDecisionSchema } from "./player-decisions.ts";
import { PLAYER_ITEMS, uniquePlayerValues } from "./player-actions.ts";
import {
  playerCounterSchema,
  playerSeatViewSchema,
  playerVisibleSubjectSchema,
  playerObjectSchema,
  playerZoneSchema,
  playerTurnSchema,
  playerStackEntrySchema,
  playerSnapshotSchema,
  validPlayerSnapshot,
  type PlayerSnapshot,
  type PlayerProjectionFacts,
} from "./player-views.ts";
export const playerTerminalSchema = z
  .strictObject({
    outcome: z.enum(["win", "draw", "no_contest"]),
    winnerSeatIds: z.array(identifierSchema).max(6),
  })
  .refine(
    (r) =>
      uniquePlayerValues(r.winnerSeatIds, (x) => x) &&
      (r.outcome === "win"
        ? r.winnerSeatIds.length > 0
        : r.winnerSeatIds.length === 0),
  );
export type PlayerTerminal = z.infer<typeof playerTerminalSchema>;
export const playerPublicChangeSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("life"),
    seatId: identifierSchema,
    before: rulesIntegerSchema,
    after: rulesIntegerSchema,
  }),
  z.strictObject({
    kind: z.literal("counters"),
    subject: playerVisibleSubjectSchema,
    counters: z.array(playerCounterSchema).max(PLAYER_ITEMS),
  }),
  // Full public seat values cover mana, poison, elimination and count changes.
  z.strictObject({
    kind: z.literal("replace-seat"),
    seat: playerSeatViewSchema,
  }),
  z.strictObject({
    kind: z.literal("upsert-object"),
    object: playerObjectSchema,
  }),
  z.strictObject({
    kind: z.literal("remove-object"),
    handle: playerHandleSchema,
  }),
  z.strictObject({ kind: z.literal("replace-zone"), zone: playerZoneSchema }),
  z.strictObject({
    kind: z.literal("turn"),
    turn: playerTurnSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("stack"),
    entries: z.array(playerStackEntrySchema).max(PLAYER_ITEMS),
  }),
  z.strictObject({
    kind: z.literal("decisions"),
    decisions: z.array(playerDecisionSchema).max(PLAYER_ITEMS),
  }),
  z.strictObject({ kind: z.literal("terminal"), result: playerTerminalSchema }),
]);
export const playerVisibleEventSchema = z.strictObject({
  basis: playerBasisSchema,
  viewerSeatId: identifierSchema,
  eventSequence: amountSchema,
  changes: z.array(playerPublicChangeSchema).min(1).max(PLAYER_ITEMS),
});
export type PlayerVisibleEvent = z.infer<typeof playerVisibleEventSchema>;
/** Historical source projections require actual archive/visibility qualification. */
export type PlayerEventFacts = {
  before: PlayerSnapshot;
  beforeProjection: PlayerProjectionFacts;
  after: PlayerSnapshot;
  afterProjection: PlayerProjectionFacts;
  terminal: PlayerTerminal | null;
};
/** Checks source-correlated public observations, not completeness or native reexecution. */
export function validPlayerVisibleEvent(
  inputEvent: PlayerVisibleEvent,
  inputFacts: PlayerEventFacts,
): boolean {
  const parsed = playerVisibleEventSchema.safeParse(inputEvent);
  const before = playerSnapshotSchema.safeParse(inputFacts.before);
  const after = playerSnapshotSchema.safeParse(inputFacts.after);
  if (!parsed.success || !before.success || !after.success) return false;
  const event = parsed.data;
  const f = { ...inputFacts, before: before.data, after: after.data };
  if (
    !validPlayerSnapshot(f.before, f.beforeProjection) ||
    !validPlayerSnapshot(f.after, f.afterProjection) ||
    !samePlayerBasis(event.basis, f.after.basis) ||
    event.viewerSeatId !== f.after.viewerSeatId ||
    event.viewerSeatId !== f.before.viewerSeatId ||
    event.eventSequence !== f.after.throughEventSequence ||
    BigInt(f.before.throughEventSequence) >= BigInt(event.eventSequence) ||
    f.before.basis.gameId !== event.basis.gameId ||
    f.before.basis.contextId !== event.basis.contextId ||
    !samePlayerWireValue(f.beforeProjection.roster, f.afterProjection.roster) ||
    playerBasisRegresses(f.before.basis, event.basis)
  )
    return false;
  return event.changes.every((change) => {
    switch (change.kind) {
      case "life":
        return (
          f.before.players.some(
            (p) => p.seatId === change.seatId && p.life === change.before,
          ) &&
          f.after.players.some(
            (p) => p.seatId === change.seatId && p.life === change.after,
          )
        );
      case "counters": {
        const subject = change.subject;
        return subject.kind === "seat"
          ? f.after.players.some(
              (p) =>
                p.seatId === subject.seatId &&
                samePlayerWireValue(p.counters, change.counters),
            )
          : f.after.visibleObjects.some(
              (o) =>
                o.handle === subject.handle &&
                samePlayerWireValue(o.counters, change.counters),
            );
      }
      case "upsert-object":
        return f.after.visibleObjects.some((o) =>
          samePlayerWireValue(o, change.object),
        );
      case "replace-seat":
        return (
          f.after.players.some((seat) =>
            samePlayerWireValue(seat, change.seat),
          ) &&
          f.before.players.some(
            (seat) =>
              seat.seatId === change.seat.seatId &&
              !samePlayerWireValue(seat, change.seat),
          )
        );
      case "remove-object":
        return (
          f.before.visibleObjects.some((o) => o.handle === change.handle) &&
          !f.after.visibleObjects.some((o) => o.handle === change.handle)
        );
      case "replace-zone":
        return f.after.zones.some((zone) =>
          samePlayerWireValue(zone, change.zone),
        );
      case "turn":
        return samePlayerWireValue(f.after.turn, change.turn);
      case "stack":
        return samePlayerWireValue(f.after.stack, change.entries);
      case "decisions":
        return samePlayerWireValue(f.after.decisions, change.decisions);
      case "terminal":
        return (
          f.terminal !== null &&
          playerTerminalSchema.safeParse(f.terminal).success &&
          samePlayerWireValue(change.result, f.terminal) &&
          change.result.winnerSeatIds.every((id) =>
            f.afterProjection.roster.includes(id),
          )
        );
    }
  });
}
