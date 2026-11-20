/** Sparse history scan records. Event sequences are not native revisions. */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";
import {
  nativeRevisionSchema,
  samePlayerWireValue,
  playerBasisRegresses,
} from "./player-primitives.ts";
import {
  playerCursorSchema,
  validPlayerCursor,
  type PlayerCursor,
} from "./player-reconnect.ts";
import {
  playerVisibleEventSchema,
  validPlayerVisibleEvent,
  type PlayerEventFacts,
} from "./player-history.ts";
export const playerHistoryQuerySchema = z.strictObject({
  gameId: identifierSchema,
  contextId: identifierSchema,
  cursor: playerCursorSchema.nullable(),
  limit: nativeRevisionSchema.refine(
    (x) => BigInt(x) >= 1n && BigInt(x) <= 100n,
  ),
});
export type PlayerHistoryQuery = z.infer<typeof playerHistoryQuerySchema>;
/** Issued cursor must be independently verified by the actual reader. */
export function validPlayerHistoryQuery(
  query: PlayerHistoryQuery,
  viewer: string,
  issued: PlayerCursor | null,
): boolean {
  if (
    !identifierSchema.safeParse(viewer).success ||
    !playerHistoryQuerySchema.safeParse(query).success
  )
    return false;
  return query.cursor === null
    ? issued === null
    : issued !== null &&
        validPlayerCursor(query.cursor, issued, viewer) &&
        query.cursor.basis.gameId === query.gameId &&
        query.cursor.basis.contextId === query.contextId;
}
export const playerHistoryPageSchema = z.strictObject({
  query: playerHistoryQuerySchema,
  viewerSeatId: identifierSchema,
  events: z.array(playerVisibleEventSchema).max(100),
  scannedThrough: playerCursorSchema,
  hasMore: z.boolean(),
});
export type PlayerHistoryPage = z.infer<typeof playerHistoryPageSchema>;
/** Actual historical source facts, not a caller-supplied claim of visibility. */
export type PlayerHistoryFacts = {
  viewer: string;
  requestedCursor: PlayerCursor | null;
  scannedCursor: PlayerCursor;
  hasMore: boolean;
  events: readonly PlayerEventFacts[];
};
/** Checks every returned observation; does not prove completeness of the source scan. */
export function validPlayerHistoryPage(
  page: PlayerHistoryPage,
  original: PlayerHistoryQuery,
  f: PlayerHistoryFacts,
): boolean {
  if (
    !playerHistoryPageSchema.safeParse(page).success ||
    !validPlayerHistoryQuery(original, f.viewer, f.requestedCursor) ||
    !validPlayerCursor(page.scannedThrough, f.scannedCursor, f.viewer) ||
    !samePlayerWireValue(page.query, original) ||
    page.viewerSeatId !== f.viewer ||
    page.scannedThrough.basis.gameId !== original.gameId ||
    page.scannedThrough.basis.contextId !== original.contextId ||
    page.hasMore !== f.hasMore ||
    BigInt(page.events.length) > BigInt(original.limit) ||
    page.events.length !== f.events.length
  )
    return false;
  const scan = page.scannedThrough;
  if (
    original.cursor !== null &&
    (BigInt(original.cursor.throughEventSequence) >
      BigInt(scan.throughEventSequence) ||
      playerBasisRegresses(original.cursor.basis, scan.basis))
  )
    return false;
  let previous = BigInt(original.cursor?.throughEventSequence ?? "0");
  let previousBasis = original.cursor?.basis;
  if (page.hasMore && previous === BigInt(scan.throughEventSequence))
    return false;
  return page.events.every((event, i) => {
    const facts = f.events[i];
    if (
      facts === undefined ||
      BigInt(event.eventSequence) <= previous ||
      BigInt(event.eventSequence) > BigInt(scan.throughEventSequence) ||
      event.basis.gameId !== original.gameId ||
      event.basis.contextId !== original.contextId ||
      event.viewerSeatId !== f.viewer ||
      playerBasisRegresses(event.basis, scan.basis) ||
      (previousBasis !== undefined &&
        playerBasisRegresses(previousBasis, event.basis)) ||
      !validPlayerVisibleEvent(event, facts)
    )
      return false;
    previous = BigInt(event.eventSequence);
    previousBasis = event.basis;
    return true;
  });
}
