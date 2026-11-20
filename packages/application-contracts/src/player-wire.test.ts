import { describe, expect, test } from "bun:test";
import { z } from "zod";
import artifact from "../fixtures/player-wire.json";
import {
  nativeRevisionSchema,
  playerBasisSchema,
  playerHandleSchema,
  rulesCountSchema,
  rulesIntegerSchema,
  playerCommandSchema,
  playerDecisionSchema,
  playerChoicePageSchema,
  playerChoiceQuerySchema,
  validPlayerDecision,
  validPlayerChoice,
  validPlayerChoicePage,
  playerSnapshotSchema,
  playerAbilityKindSchema,
  playerEnglishLabelSchema,
  validPlayerSnapshot,
  playerReceiptSchema,
  playerReceiptQuerySchema,
  validPlayerReceipt,
  validPlayerReceipts,
  playerCursorSchema,
  validPlayerCursor,
  playerReconnectRequestSchema,
  validPlayerReconnectRequest,
  playerReconnectSchema,
  validPlayerReconnect,
  playerVisibleEventSchema,
  playerTerminalSchema,
  validPlayerVisibleEvent,
  playerHistoryQuerySchema,
  playerHistoryPageSchema,
  validPlayerHistoryQuery,
  validPlayerHistoryPage,
  playerPresentationSchema,
  playerStreamFrameSchema,
  playerStreamRequestSchema,
  validPlayerStreamRequest,
  validPlayerStreamFrame,
  playerReplayCodecSchema,
  playerReplayModeSchema,
  playerReplayRequestSchema,
  playerReplayFrameSchema,
  validPlayerReplayRequest,
  validPlayerReplayFrame,
} from "./player.ts";
import { amountSchema, identifierSchema } from "./primitives.ts";
import { parseStrictJson } from "./raw-json.ts";
const baseContext = z.strictObject({
  basis: playerBasisSchema,
  viewer: identifierSchema,
  roster: z.array(identifierSchema),
  descriptor: playerDecisionSchema.optional(),
  query: playerChoiceQuerySchema.optional(),
  pages: z.array(playerChoicePageSchema).default([]),
  authorizedFullDisclosure: z.boolean().default(false),
  throughSequence: amountSchema.optional(),
  visibleObjects: z.array(playerHandleSchema).default([]),
  knownIdentities: z.array(playerHandleSchema).default([]),
  orderedZones: z.array(playerHandleSchema).default([]),
  creatureTypeMembers: z.array(playerEnglishLabelSchema).default([]),
  abilityBindings: z
    .array(
      z.strictObject({
        source: playerHandleSchema,
        handle: playerHandleSchema,
        kind: playerAbilityKindSchema,
      }),
    )
    .default([]),
  original: playerCommandSchema.optional(),
  receiptQuery: playerReceiptQuerySchema.optional(),
  reconnectRequest: playerReconnectRequestSchema.optional(),
  issuedCursor: playerCursorSchema.nullable().optional(),
  snapshotCursor: playerCursorSchema.optional(),
  historyQuery: playerHistoryQuerySchema.optional(),
  scannedCursor: playerCursorSchema.optional(),
  hasMore: z.boolean().default(false),
  previousCursor: playerCursorSchema.nullable().optional(),
  presentation: playerPresentationSchema.optional(),
  streamGap: z.boolean().default(false),
  expectedEffect: z
    .enum(["apply-snapshot", "refresh-presentation", "require-snapshot"])
    .optional(),
  replayRequest: playerReplayRequestSchema.optional(),
  replayAuth: z
    .strictObject({
      requesterAccountId: identifierSchema,
      participantAccountIds: z.array(identifierSchema),
      historicalSeatId: identifierSchema,
      historicalSeatOwner: identifierSchema.nullable(),
      completed: z.boolean(),
      fullInformationEligible: z.boolean(),
      sourceGameId: identifierSchema,
      sourceContextId: identifierSchema,
      releaseId: identifierSchema,
      schemaRevision: playerReplayCodecSchema,
    })
    .optional(),
});
const eventFactsSchema = z.strictObject({
  before: playerSnapshotSchema,
  after: playerSnapshotSchema,
  beforeContext: baseContext,
  afterContext: baseContext,
  terminal: playerTerminalSchema.nullable(),
});
const context = baseContext.extend({
  eventFacts: eventFactsSchema.optional(),
  historyEvents: z.array(eventFactsSchema).default([]),
});
function eventFacts(f: z.infer<typeof eventFactsSchema>) {
  const beforeSequence = f.beforeContext.throughSequence;
  const afterSequence = f.afterContext.throughSequence;
  if (beforeSequence === undefined || afterSequence === undefined)
    throw new Error("missing historical fixture frontier");
  return {
    before: f.before,
    after: f.after,
    beforeProjection: { ...f.beforeContext, throughSequence: beforeSequence },
    afterProjection: { ...f.afterContext, throughSequence: afterSequence },
    terminal: f.terminal,
  };
}
describe("shared original player wire", () => {
  for (const fixture of artifact.cases) {
    test(fixture.name, () => {
      const raw = parseStrictJson(fixture.wire);
      let valid = false;
      if (raw.success) {
        const ctx = context.safeParse(
          "context" in fixture ? fixture.context : undefined,
        );
        switch (fixture.kind) {
          case "replay_schema":
            valid = playerReplayCodecSchema.safeParse(raw.value).success;
            break;
          case "replay_mode":
            valid = playerReplayModeSchema.safeParse(raw.value).success;
            break;
          case "replay_request": {
            const p = playerReplayRequestSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              validPlayerReplayRequest(p.data, ctx.data.issuedCursor ?? null);
            break;
          }
          case "replay_frame": {
            const p = playerReplayFrameSchema.safeParse(raw.value);
            if (
              p.success &&
              ctx.success &&
              ctx.data.replayAuth !== undefined &&
              ctx.data.replayRequest !== undefined &&
              ctx.data.scannedCursor !== undefined &&
              ctx.data.throughSequence !== undefined
            ) {
              const c = ctx.data;
              const a = ctx.data.replayAuth;
              valid = validPlayerReplayFrame(p.data, ctx.data.replayRequest, {
                requesterAccountId: a.requesterAccountId,
                participantAccountIds: a.participantAccountIds,
                historicalSeatId: a.historicalSeatId,
                historicalSeatOwner: a.historicalSeatOwner,
                completed: a.completed,
                fullInformationEligible: a.fullInformationEligible,
                source: {
                  gameId: a.sourceGameId,
                  contextId: a.sourceContextId,
                  releaseId: a.releaseId,
                  schemaRevision: a.schemaRevision,
                },
                projection: { ...c, throughSequence: ctx.data.throughSequence },
                history: {
                  viewer: c.viewer,
                  requestedCursor: c.issuedCursor ?? null,
                  scannedCursor: ctx.data.scannedCursor,
                  hasMore: c.hasMore,
                  events: c.historyEvents.map(eventFacts),
                },
              });
            }
            break;
          }
          case "rules_integer":
            valid = rulesIntegerSchema.safeParse(raw.value).success;
            break;
          case "rules_count":
            valid = rulesCountSchema.safeParse(raw.value).success;
            break;
          case "native_revision":
            valid = nativeRevisionSchema.safeParse(raw.value).success;
            break;
          case "handle":
            valid = playerHandleSchema.safeParse(raw.value).success;
            break;
          case "basis":
            valid = playerBasisSchema.safeParse(raw.value).success;
            break;
          case "command":
            valid = playerCommandSchema.safeParse(raw.value).success;
            break;
          case "decision": {
            const p = playerDecisionSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              validPlayerDecision(
                p.data,
                ctx.data.basis,
                ctx.data.viewer,
                ctx.data.roster,
                ctx.data.authorizedFullDisclosure,
              );
            break;
          }
          case "choice": {
            const p = playerCommandSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.descriptor !== undefined &&
              validPlayerChoice(
                ctx.data.descriptor,
                p.data,
                ctx.data.viewer,
                ctx.data.roster,
                ctx.data.pages,
              );
            if (valid && p.success) expect(p.data).toEqual(raw.value);
            break;
          }
          case "choice_query":
            valid = playerChoiceQuerySchema.safeParse(raw.value).success;
            break;
          case "choice_page": {
            const p = playerChoicePageSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.query !== undefined &&
              validPlayerChoicePage(p.data, ctx.data.query, ctx.data.viewer);
            break;
          }
          case "snapshot": {
            const p = playerSnapshotSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.throughSequence !== undefined &&
              validPlayerSnapshot(p.data, {
                ...ctx.data,
                throughSequence: ctx.data.throughSequence,
              });
            break;
          }
          case "receipt": {
            const p = playerReceiptSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.original !== undefined &&
              validPlayerReceipt(p.data, ctx.data.original, ctx.data.viewer);
            break;
          }
          case "event": {
            const p = playerVisibleEventSchema.safeParse(raw.value);
            if (p.success && ctx.success && ctx.data.eventFacts !== undefined) {
              valid = validPlayerVisibleEvent(
                p.data,
                eventFacts(ctx.data.eventFacts),
              );
            }
            break;
          }
          case "receipt_query":
            valid = playerReceiptQuerySchema.safeParse(raw.value).success;
            break;
          case "history_query": {
            const p = playerHistoryQuerySchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              validPlayerHistoryQuery(
                p.data,
                ctx.data.viewer,
                ctx.data.issuedCursor ?? null,
              );
            break;
          }
          case "stream": {
            const p = playerStreamFrameSchema.safeParse(raw.value);
            if (
              p.success &&
              ctx.success &&
              ctx.data.throughSequence !== undefined
            ) {
              const c = ctx.data;
              const facts =
                c.streamGap &&
                c.issuedCursor !== undefined &&
                c.issuedCursor !== null
                  ? { kind: "gap" as const, requestedCursor: c.issuedCursor }
                  : c.presentation !== undefined &&
                      c.snapshotCursor !== undefined
                    ? {
                        kind: "snapshot" as const,
                        projection: {
                          ...c,
                          throughSequence: ctx.data.throughSequence,
                        },
                        issuedCursor: c.snapshotCursor,
                        presentation: c.presentation,
                      }
                    : null;
              if (facts !== null) {
                const effect = validPlayerStreamFrame(
                  p.data,
                  c.previousCursor ?? null,
                  facts,
                );
                valid = effect !== null;
                if (effect !== null) expect(effect).toBe(c.expectedEffect);
              }
            }
            break;
          }
          case "history_page": {
            const p = playerHistoryPageSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.historyQuery !== undefined &&
              ctx.data.scannedCursor !== undefined &&
              validPlayerHistoryPage(p.data, ctx.data.historyQuery, {
                viewer: ctx.data.viewer,
                requestedCursor: ctx.data.issuedCursor ?? null,
                scannedCursor: ctx.data.scannedCursor,
                hasMore: ctx.data.hasMore,
                events: ctx.data.historyEvents.map(eventFacts),
              });
            break;
          }
          case "cursor": {
            const p = playerCursorSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.issuedCursor !== undefined &&
              ctx.data.issuedCursor !== null &&
              validPlayerCursor(p.data, ctx.data.issuedCursor, ctx.data.viewer);
            break;
          }
          case "stream_request": {
            const p = playerStreamRequestSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              validPlayerStreamRequest(
                p.data,
                ctx.data.viewer,
                ctx.data.issuedCursor ?? null,
              );
            break;
          }
          case "reconnect_request": {
            const p = playerReconnectRequestSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              validPlayerReconnectRequest(
                p.data,
                ctx.data.viewer,
                ctx.data.issuedCursor ?? null,
              );
            break;
          }
          case "reconnect": {
            const p = playerReconnectSchema.safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.reconnectRequest !== undefined &&
              ctx.data.snapshotCursor !== undefined &&
              ctx.data.throughSequence !== undefined &&
              validPlayerReconnect(p.data, ctx.data.reconnectRequest, {
                projection: {
                  ...ctx.data,
                  throughSequence: ctx.data.throughSequence,
                },
                requestedCursor: ctx.data.issuedCursor ?? null,
                snapshotCursor: ctx.data.snapshotCursor,
              });
            break;
          }
          case "receipt_results": {
            const p = z
              .strictObject({ receipts: z.array(playerReceiptSchema) })
              .safeParse(raw.value);
            valid =
              p.success &&
              ctx.success &&
              ctx.data.receiptQuery !== undefined &&
              validPlayerReceipts(
                ctx.data.receiptQuery,
                p.data.receipts,
                ctx.data.viewer,
              );
            break;
          }
          default:
            throw new Error("Unknown maintained player fixture kind");
        }
      }
      expect(valid).toBe(fixture.valid);
    });
  }
});
