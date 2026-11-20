/** Declared stream records; no socket, publisher or permission-renewal runtime. */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";
import { appearanceSchema } from "./appearance.ts";
import { samePlayerWireValue } from "./player-primitives.ts";
import { uniquePlayerValues } from "./player-actions.ts";
import {
  playerSnapshotSchema,
  type PlayerProjectionFacts,
} from "./player-views.ts";
import {
  playerCursorSchema,
  validPlayerCursor,
  validPlayerRequestedCursor,
  validPlayerCursorProgress,
  validPlayerSnapshotCursor,
  type PlayerCursor,
} from "./player-reconnect.ts";
/** Explicit null starts fresh; syntax never establishes cursor issuance or authority. */
export const playerStreamRequestSchema = z.strictObject({
  gameId: identifierSchema,
  contextId: identifierSchema,
  cursor: playerCursorSchema.nullable(),
});
export type PlayerStreamRequest = z.infer<typeof playerStreamRequestSchema>;
/** Issued must be independently read from the trusted viewer-scoped cursor source. */
export function validPlayerStreamRequest(
  request: PlayerStreamRequest,
  viewer: string,
  issued: PlayerCursor | null,
): boolean {
  return (
    playerStreamRequestSchema.safeParse(request).success &&
    identifierSchema.safeParse(viewer).success &&
    validPlayerRequestedCursor(
      request.cursor,
      issued,
      viewer,
      request.gameId,
      request.contextId,
    )
  );
}
export const playerSeatAppearanceSchema = z.strictObject({
  seatId: identifierSchema,
  appearance: appearanceSchema,
});
export const playerPresentationSchema = z.strictObject({
  seats: z.array(playerSeatAppearanceSchema).min(1).max(6),
});
export type PlayerPresentation = z.infer<typeof playerPresentationSchema>;
/** Resolved is from current trusted entitlement lookup, never client grant evidence. */
export function validPlayerPresentation(
  presentation: PlayerPresentation,
  roster: readonly string[],
  resolved: PlayerPresentation,
): boolean {
  return (
    playerPresentationSchema.safeParse(presentation).success &&
    playerPresentationSchema.safeParse(resolved).success &&
    roster.length >= 1 &&
    roster.length <= 6 &&
    uniquePlayerValues(roster, (x) => x) &&
    roster.every((id) => identifierSchema.safeParse(id).success) &&
    samePlayerWireValue(
      presentation.seats.map((s) => s.seatId),
      roster,
    ) &&
    samePlayerWireValue(presentation, resolved)
  );
}
export const playerStreamFrameSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("snapshot"),
    snapshot: playerSnapshotSchema,
    cursor: playerCursorSchema,
    presentation: playerPresentationSchema,
  }),
  z.strictObject({ kind: z.literal("gap"), cursor: playerCursorSchema }),
]);
export type PlayerStreamFrame = z.infer<typeof playerStreamFrameSchema>;
/** Independently obtained source facts, not a serialized authorization claim. */
export type PlayerStreamFacts =
  | {
      kind: "snapshot";
      projection: PlayerProjectionFacts;
      issuedCursor: PlayerCursor;
      presentation: PlayerPresentation;
    }
  | { kind: "gap"; requestedCursor: PlayerCursor };
export type PlayerStreamEffect =
  "apply-snapshot" | "refresh-presentation" | "require-snapshot";
/** Duplicate board delivery still refreshes cosmetics while gameplay is idle. */
export function validPlayerStreamFrame(
  frame: PlayerStreamFrame,
  previous: PlayerCursor | null,
  facts: PlayerStreamFacts,
): PlayerStreamEffect | null {
  if (!playerStreamFrameSchema.safeParse(frame).success) return null;
  if (frame.kind === "snapshot" && facts.kind === "snapshot") {
    if (
      !validPlayerSnapshotCursor(
        frame.snapshot,
        frame.cursor,
        facts.projection,
        facts.issuedCursor,
      ) ||
      !validPlayerPresentation(
        frame.presentation,
        facts.projection.roster,
        facts.presentation,
      ) ||
      (previous !== null && !validPlayerCursorProgress(frame.cursor, previous))
    )
      return null;
    return previous !== null && samePlayerWireValue(frame.cursor, previous)
      ? "refresh-presentation"
      : "apply-snapshot";
  }
  if (
    frame.kind === "gap" &&
    facts.kind === "gap" &&
    previous !== null &&
    samePlayerWireValue(previous, facts.requestedCursor) &&
    validPlayerCursor(
      frame.cursor,
      facts.requestedCursor,
      facts.requestedCursor.viewerSeatId,
    )
  )
    return "require-snapshot";
  return null;
}
