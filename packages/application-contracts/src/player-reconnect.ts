/** Read-only reconnect records. Cursor syntax is never cursor issuance/authentication. */
import { z } from "zod";
import { amountSchema, identifierSchema } from "./primitives.ts";
import {
  playerBasisSchema,
  playerHandleSchema,
  playerBasisRegresses,
} from "./player-primitives.ts";
import { samePlayerBasis } from "./player-decisions.ts";
import {
  playerSnapshotSchema,
  validPlayerSnapshot,
  type PlayerProjectionFacts,
} from "./player-views.ts";
import {
  playerReceiptQuerySchema,
  playerReceiptSchema,
  validPlayerReceipts,
} from "./player-receipts.ts";

export const playerCursorSchema = z.strictObject({
  basis: playerBasisSchema,
  viewerSeatId: identifierSchema,
  throughEventSequence: amountSchema,
  token: playerHandleSchema,
});
export type PlayerCursor = z.infer<typeof playerCursorSchema>;
/** Compare verified coordinates; cross-epoch lineage remains source-owned. */
export function validPlayerCursorProgress(
  cursor: PlayerCursor,
  previous: PlayerCursor,
): boolean {
  return (
    cursor.viewerSeatId === previous.viewerSeatId &&
    cursor.basis.gameId === previous.basis.gameId &&
    cursor.basis.contextId === previous.basis.contextId &&
    BigInt(cursor.throughEventSequence) >=
      BigInt(previous.throughEventSequence) &&
    !playerBasisRegresses(previous.basis, cursor.basis)
  );
}
/** One accepted snapshot and issued cursor describe exactly the same view/frontier. */
export function validPlayerSnapshotCursor(
  snapshot: z.infer<typeof playerSnapshotSchema>,
  cursor: PlayerCursor,
  projection: PlayerProjectionFacts,
  issued: PlayerCursor,
): boolean {
  return (
    validPlayerSnapshot(snapshot, projection) &&
    validPlayerCursor(cursor, issued, projection.viewer) &&
    samePlayerBasis(cursor.basis, snapshot.basis) &&
    cursor.throughEventSequence === snapshot.throughEventSequence
  );
}
/** Expected cursor is independently recovered from a qualified issuer, not copied from input. */
export function validPlayerCursor(
  cursor: PlayerCursor,
  issued: PlayerCursor,
  viewer: string,
): boolean {
  return (
    identifierSchema.safeParse(viewer).success &&
    playerCursorSchema.safeParse(cursor).success &&
    playerCursorSchema.safeParse(issued).success &&
    cursor.viewerSeatId === viewer &&
    cursor.viewerSeatId === issued.viewerSeatId &&
    samePlayerBasis(cursor.basis, issued.basis) &&
    cursor.throughEventSequence === issued.throughEventSequence &&
    cursor.token === issued.token
  );
}
export const playerReconnectRequestSchema = z.strictObject({
  receipts: playerReceiptQuerySchema,
  cursor: playerCursorSchema.nullable(),
});
export type PlayerReconnectRequest = z.infer<
  typeof playerReconnectRequestSchema
>;
/** No move is resubmitted, including when receipt status is unknown. */
export function validPlayerReconnectRequest(
  request: PlayerReconnectRequest,
  viewer: string,
  issued: PlayerCursor | null,
): boolean {
  if (
    !identifierSchema.safeParse(viewer).success ||
    !playerReconnectRequestSchema.safeParse(request).success
  )
    return false;
  return validPlayerRequestedCursor(
    request.cursor,
    issued,
    viewer,
    request.receipts.gameId,
    request.receipts.contextId,
  );
}
/** Shared optional issued-cursor identity/scope check for reconnect and stream intake. */
export function validPlayerRequestedCursor(
  requested: PlayerCursor | null,
  issued: PlayerCursor | null,
  viewer: string,
  gameId: string,
  contextId: string,
): boolean {
  if (!identifierSchema.safeParse(viewer).success) return false;
  if (requested === null) return issued === null;
  return (
    issued !== null &&
    validPlayerCursor(requested, issued, viewer) &&
    requested.basis.gameId === gameId &&
    requested.basis.contextId === contextId
  );
}

export const playerReconnectSchema = z.strictObject({
  snapshot: playerSnapshotSchema,
  cursor: playerCursorSchema,
  receipts: z.array(playerReceiptSchema).max(100),
});
export type PlayerReconnect = z.infer<typeof playerReconnectSchema>;
/** Trusted source facts require real authorization, lineage and cursor verification. */
export type PlayerReconnectFacts = {
  projection: PlayerProjectionFacts;
  requestedCursor: PlayerCursor | null;
  snapshotCursor: PlayerCursor;
};
/** Snapshot and receipt lookups are independently observed, not a single atomic read. */
export function validPlayerReconnect(
  response: PlayerReconnect,
  request: PlayerReconnectRequest,
  facts: PlayerReconnectFacts,
): boolean {
  return (
    playerReconnectSchema.safeParse(response).success &&
    validPlayerReconnectRequest(
      request,
      facts.projection.viewer,
      facts.requestedCursor,
    ) &&
    validPlayerSnapshotCursor(
      response.snapshot,
      response.cursor,
      facts.projection,
      facts.snapshotCursor,
    ) &&
    response.snapshot.basis.gameId === request.receipts.gameId &&
    response.snapshot.basis.contextId === request.receipts.contextId &&
    (request.cursor === null ||
      validPlayerCursorProgress(response.cursor, request.cursor)) &&
    validPlayerReceipts(
      request.receipts,
      response.receipts,
      facts.projection.viewer,
    )
  );
}
