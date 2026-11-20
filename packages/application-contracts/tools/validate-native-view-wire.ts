/** Validate actual owned Rust HTTP payloads with the maintained client wire schemas.
 * Structural/correlation evidence only, not an independent native visibility proof.
 */
import { z } from "zod";
import { readFile } from "node:fs/promises";
import { argv } from "node:process";
import {
  apiResponseSchema,
  gamePageSchema,
  matchDetailSchema,
  playerViewRequestSchema,
  playerViewSchema,
  playerReconnectRequestSchema,
  playerReconnectSchema,
  playerStreamRequestSchema,
  playerStreamFrameSchema,
} from "../src/index.ts";

const path = argv[2];
if (!path) throw new Error("Missing owned HTTP capture");
const capture = z
  .strictObject({
    request: playerViewRequestSchema,
    metadata: z.array(apiResponseSchema(matchDetailSchema)).length(2),
    gamePages: z.array(apiResponseSchema(gamePageSchema)).length(2),
    responses: z.array(apiResponseSchema(playerViewSchema)).length(2),
    streams: z
      .array(
        z.strictObject({
          request: playerStreamRequestSchema,
          frames: z
            .array(apiResponseSchema(playerStreamFrameSchema))
            .min(1)
            .max(2),
        }),
      )
      .length(3),
    reconnect: z
      .array(
        z.strictObject({
          request: playerReconnectRequestSchema,
          response: apiResponseSchema(playerReconnectSchema),
        }),
      )
      .length(4),
  })
  .parse(JSON.parse(await readFile(path, "utf8")));
const viewers = new Set<string>();
for (const [index, response] of capture.responses.entries()) {
  if (response.status !== "success")
    throw new Error("Expected actual successful participant payload");
  const snapshot = response.data.snapshot;
  if (
    snapshot.basis.gameId !== capture.request.gameId ||
    snapshot.basis.contextId !== capture.request.contextId
  ) {
    throw new Error("Actual view scope differs from request");
  }
  const metadata = capture.metadata[index];
  const games = capture.gamePages[index];
  if (
    !metadata ||
    metadata.status !== "success" ||
    !games ||
    games.status !== "success"
  )
    throw new Error("Missing successful public bootstrap");
  const selected = games.data.items.find(
    (item) => item.id === snapshot.basis.gameId,
  );
  if (
    metadata.data.match.id !== games.data.matchId ||
    metadata.data.accountId !== games.data.accountId ||
    metadata.data.match.currentGameId !== snapshot.basis.gameId ||
    metadata.data.ownSeatId !== snapshot.viewerSeatId ||
    !selected ||
    selected.contextId !== snapshot.basis.contextId
  )
    throw new Error(
      "View scope did not come from the matching public participant game page",
    );
  viewers.add(snapshot.viewerSeatId);
}
if (viewers.size !== 2)
  throw new Error("Actual views did not retain distinct participant scopes");
for (const item of capture.reconnect) {
  if (item.response.status !== "success")
    throw new Error("Expected actual reconnect success");
  const data = item.response.data,
    query = item.request.receipts;
  if (
    data.snapshot.basis.gameId !== query.gameId ||
    data.snapshot.basis.contextId !== query.contextId ||
    data.cursor.viewerSeatId !== data.snapshot.viewerSeatId ||
    JSON.stringify(data.cursor.basis) !== JSON.stringify(data.snapshot.basis) ||
    data.cursor.throughEventSequence !== data.snapshot.throughEventSequence ||
    data.receipts.length !== query.outstanding.length
  )
    throw new Error("Reconnect scope/frontier differs");
  for (const [index, receipt] of data.receipts.entries()) {
    const requested = query.outstanding[index];
    if (
      !requested ||
      JSON.stringify(receipt.original) !== JSON.stringify(requested) ||
      receipt.actorSeatId !== data.snapshot.viewerSeatId
    )
      throw new Error("Reconnect original correlation differs");
  }
}
for (const stream of capture.streams) {
  for (const record of stream.frames) {
    if (record.status !== "success" || record.data.kind !== "snapshot") {
      throw new Error("Expected actual stream snapshot success");
    }
    const { snapshot, cursor, presentation } = record.data;
    if (
      snapshot.basis.gameId !== stream.request.gameId ||
      snapshot.basis.contextId !== stream.request.contextId ||
      cursor.viewerSeatId !== snapshot.viewerSeatId ||
      JSON.stringify(cursor.basis) !== JSON.stringify(snapshot.basis) ||
      cursor.throughEventSequence !== snapshot.throughEventSequence ||
      !viewers.has(snapshot.viewerSeatId) ||
      JSON.stringify(presentation.seats.map((seat) => seat.seatId)) !==
        JSON.stringify(snapshot.players.map((player) => player.seatId))
    ) {
      throw new Error(
        "Stream scope/frontier/current presentation roster differs",
      );
    }
  }
}
console.info("NATIVE_VIEW_RECONNECT_STREAM_HTTP_WIRE_OK 2 4 3");
