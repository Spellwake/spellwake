/** Participant replay records. No private archive reader or native replay codec. */
import { z } from "zod";
import { amountSchema, identifierSchema } from "./primitives.ts";
import {
  playerBasisRegresses,
  samePlayerWireValue,
} from "./player-primitives.ts";
import { uniquePlayerValues } from "./player-actions.ts";
import {
  playerSnapshotSchema,
  validPlayerSnapshot,
  type PlayerProjectionFacts,
} from "./player-views.ts";
import { type PlayerCursor } from "./player-reconnect.ts";
import {
  playerHistoryQuerySchema,
  playerHistoryPageSchema,
  validPlayerHistoryQuery,
  validPlayerHistoryPage,
  type PlayerHistoryFacts,
} from "./player-history-page.ts";
/** Public codec label, distinct from private schema proofs and artifact locators. */
export const playerReplayCodecSchema = z
  .string()
  .min(1)
  .max(128)
  .refine(
    (s) =>
      [...s].every((c) => /^[A-Za-z0-9._-]$/.test(c)) &&
      /^[A-Za-z0-9]$/.test(s[0] ?? ""),
  );
export const playerReplayModeSchema = z.enum([
  "historical-seat",
  "completed-full-information",
]);
export const playerReplayRequestSchema = z.strictObject({
  gameId: identifierSchema,
  contextId: identifierSchema,
  atEventSequence: amountSchema,
  perspectiveSeatId: identifierSchema,
  mode: playerReplayModeSchema,
  history: playerHistoryQuerySchema,
});
export type PlayerReplayRequest = z.infer<typeof playerReplayRequestSchema>;
/** Requested mode is not authorization. Original cursor must be independently verified. */
export function validPlayerReplayRequest(
  request: PlayerReplayRequest,
  issued: PlayerCursor | null,
): boolean {
  return (
    playerReplayRequestSchema.safeParse(request).success &&
    validPlayerHistoryQuery(
      request.history,
      request.perspectiveSeatId,
      issued,
    ) &&
    request.history.gameId === request.gameId &&
    request.history.contextId === request.contextId &&
    (request.history.cursor === null ||
      BigInt(request.history.cursor.throughEventSequence) <=
        BigInt(request.atEventSequence))
  );
}
export const playerReplayFrameSchema = z.strictObject({
  request: playerReplayRequestSchema,
  releaseId: identifierSchema,
  schemaRevision: playerReplayCodecSchema,
  snapshot: playerSnapshotSchema,
  history: playerHistoryPageSchema,
});
export type PlayerReplayFrame = z.infer<typeof playerReplayFrameSchema>;
/** Resolved original archive/context lineage. Never deserialize this as source proof. */
export type PlayerReplaySource = {
  gameId: string;
  contextId: string;
  releaseId: string;
  schemaRevision: string;
};
/** Verified account and retained actual participant/seat provenance, independent of perspective. */
export type PlayerReplayFacts = {
  requesterAccountId: string;
  participantAccountIds: readonly string[];
  historicalSeatId: string;
  historicalSeatOwner: string | null;
  completed: boolean;
  fullInformationEligible: boolean;
  source: PlayerReplaySource;
  projection: PlayerProjectionFacts;
  history: PlayerHistoryFacts;
};
/** Checks bounded historical presentation. Real lineage, native replay and visibility
 * qualification still belong to the actual archive reader and authenticated adapter.
 */
export function validPlayerReplayFrame(
  frame: PlayerReplayFrame,
  original: PlayerReplayRequest,
  f: PlayerReplayFacts,
): boolean {
  if (
    !playerReplayFrameSchema.safeParse(frame).success ||
    !validPlayerReplayRequest(original, f.history.requestedCursor)
  )
    return false;
  const full = original.mode === "completed-full-information";
  if (
    !identifierSchema.safeParse(f.requesterAccountId).success ||
    f.participantAccountIds.length > 6 ||
    !uniquePlayerValues(f.participantAccountIds, (x) => x) ||
    !f.participantAccountIds.every(
      (id) => identifierSchema.safeParse(id).success,
    ) ||
    !f.participantAccountIds.includes(f.requesterAccountId) ||
    f.historicalSeatId !== original.perspectiveSeatId ||
    (full
      ? !f.completed || !f.fullInformationEligible
      : f.historicalSeatOwner !== f.requesterAccountId) ||
    !samePlayerWireValue(frame.request, original) ||
    f.source.gameId !== original.gameId ||
    f.source.contextId !== original.contextId ||
    !identifierSchema.safeParse(f.source.releaseId).success ||
    frame.releaseId !== f.source.releaseId ||
    frame.schemaRevision !== f.source.schemaRevision ||
    frame.snapshot.basis.gameId !== original.gameId ||
    frame.snapshot.basis.contextId !== original.contextId ||
    frame.snapshot.viewerSeatId !== original.perspectiveSeatId ||
    frame.snapshot.throughEventSequence !== original.atEventSequence ||
    f.projection.authorizedFullDisclosure !== full ||
    f.history.viewer !== original.perspectiveSeatId ||
    BigInt(frame.history.scannedThrough.throughEventSequence) >
      BigInt(original.atEventSequence) ||
    (frame.history.hasMore &&
      frame.history.scannedThrough.throughEventSequence ===
        original.atEventSequence) ||
    playerBasisRegresses(
      frame.history.scannedThrough.basis,
      frame.snapshot.basis,
    ) ||
    f.history.events.some(
      (e) =>
        e.beforeProjection.authorizedFullDisclosure !== full ||
        e.afterProjection.authorizedFullDisclosure !== full ||
        !samePlayerWireValue(e.beforeProjection.roster, f.projection.roster) ||
        !samePlayerWireValue(e.afterProjection.roster, f.projection.roster),
    )
  )
    return false;
  return (
    validPlayerSnapshot(frame.snapshot, f.projection) &&
    validPlayerHistoryPage(frame.history, original.history, f.history)
  );
}
