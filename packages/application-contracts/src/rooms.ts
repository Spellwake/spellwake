/** Original lobby intent only; no native admission or trusted caller fields. */
import { z } from "zod";
import { validProgressionPage } from "./progression.ts";
import { identifierSchema, amountSchema, instantSchema } from "./primitives.ts";
const createRoomSchema = z
  .strictObject({
    requestId: identifierSchema,
    formatId: identifierSchema,
    profileId: identifierSchema,
    mode: z.enum(["open", "collection"]),
    competitive: z.boolean(),
    visibility: z.enum(["public", "private"]),
  })
  .refine(
    (input) => !input.competitive || input.mode === "collection",
    "Competitive play requires Collection Play",
  );
const selectRoomDeckSchema = z.strictObject({
  requestId: identifierSchema,
  roomId: identifierSchema,
  expectedRevision: amountSchema,
  deckRevisionId: identifierSchema.nullable(),
});
const leaveRoomSchema = z.strictObject({
  requestId: identifierSchema,
  roomId: identifierSchema,
  expectedRevision: amountSchema,
});
export const roomCommandSchema = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("create"), command: createRoomSchema }),
  z.strictObject({
    kind: z.literal("select_deck"),
    command: selectRoomDeckSchema,
  }),
  z.strictObject({ kind: z.literal("leave"), command: leaveRoomSchema }),
]);
export type RoomCommand = z.infer<typeof roomCommandSchema>;
export const roomCommandErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ configuration: z.literal("invalid") }),
  z.strictObject({ format: z.literal("unavailable") }),
  z.strictObject({ profile: z.literal("unavailable") }),
  z.strictObject({ room: z.literal("unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
  z.strictObject({ deck: z.literal("unavailable") }),
  z.strictObject({ queue: z.enum(["unavailable", "already reserved"]) }),
]);
/** Call after schema parsing: a receipt is valid only for its original operation and target. */
export function validRoomReceipt(
  command: RoomCommand,
  receipt:
    | { state: "unknown" | "pending" }
    | { state: "accepted"; response: { data: { resourceId: string } } }
    | {
        state: "rejected";
        response: { data: z.infer<typeof roomCommandErrorsSchema> };
      },
): boolean {
  if (receipt.state === "accepted")
    return (
      command.kind === "create" ||
      command.command.roomId === receipt.response.data.resourceId
    );
  if (receipt.state !== "rejected") return true;
  const errors = receipt.response.data;
  if ("account" in errors) return true;
  switch (command.kind) {
    case "create":
      return (
        "configuration" in errors || "format" in errors || "profile" in errors
      );
    case "select_deck":
      return "room" in errors || "revision" in errors || "deck" in errors;
    case "leave":
      return "room" in errors || "revision" in errors || "queue" in errors;
  }
}

/** Public metadata preserves source state text without inferring gameplay qualification. */
export const roomSummarySchema = z
  .strictObject({
    id: identifierSchema,
    hostId: identifierSchema,
    formatId: identifierSchema,
    profileId: identifierSchema,
    mode: z.enum(["open", "collection"]),
    competitive: z.boolean(),
    visibility: z.enum(["public", "private"]),
    state: z.string(),
    revision: amountSchema,
    structure: z.enum(["bo1", "bo3"]),
    configurationId: identifierSchema,
    targetSeatCount: amountSchema.nullable(),
    countdownAt: instantSchema.nullable(),
  })
  .refine(
    (room) =>
      (!room.competitive || room.mode === "collection") &&
      (room.targetSeatCount === null ||
        (BigInt(room.targetSeatCount) >= 2n &&
          BigInt(room.targetSeatCount) <= 6n)),
  );
export const roomSlotSchema = z
  .strictObject({
    id: identifierSchema,
    accountId: identifierSchema.nullable(),
    seatIndex: amountSchema.refine((n) => BigInt(n) <= 9223372036854775807n),
    controllerKind: z.enum(["human", "ai"]),
    ready: z.boolean(),
    deckRevisionId: identifierSchema.nullable(),
  })
  .refine((slot) => slot.controllerKind !== "ai" || slot.accountId === null);
export const roomSlotPageSchema = z.strictObject({
  items: z.array(roomSlotSchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export const roomPageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(roomSummarySchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
export const roomDetailSchema = z.strictObject({
  accountId: identifierSchema,
  room: roomSummarySchema,
  slots: roomSlotPageSchema,
});
/** A signed caller's path from a lobby seat to the current match and game. */
export const roomGameSchema = z.discriminatedUnion("state", [
  z.strictObject({
    state: z.literal("waiting"),
    accountId: identifierSchema,
    roomId: identifierSchema,
    roomState: z.enum(["lobby", "countdown"]),
  }),
  z.strictObject({
    state: z.literal("admitted"),
    accountId: identifierSchema,
    roomId: identifierSchema,
    matchId: identifierSchema,
    seatId: identifierSchema,
    game: z
      .strictObject({
        id: identifierSchema,
        state: z.enum(["reserved", "active", "completed", "no_contest", "canceled"]),
        contextId: identifierSchema.nullable(),
      })
      .nullable(),
  }),
]);

export type RoomGame = z.infer<typeof roomGameSchema>;

/** Context validation is required after shape parsing; each page is an independent snapshot. */
export function validRoomPage(
  page: z.infer<typeof roomPageSchema>,
  owner: string,
  after: string | null,
  limit: number,
): boolean {
  return (
    page.accountId === owner &&
    validProgressionPage({ ...page }, owner, after, limit)
  );
}
/** Only an occupied human slot belonging to this caller can expose a selected revision. */
export function validRoomDetail(
  detail: z.infer<typeof roomDetailSchema>,
  owner: string,
  room: string,
  after: string | null,
  limit: number,
): boolean {
  return (
    detail.accountId === owner &&
    detail.room.id === room &&
    validProgressionPage(
      { ...detail.slots, accountId: owner },
      owner,
      after,
      limit,
    ) &&
    new Set(detail.slots.items.map((s) => s.seatIndex)).size ===
      detail.slots.items.length &&
    new Set(
      detail.slots.items.flatMap((slot) =>
        slot.accountId === null ? [] : [slot.accountId],
      ),
    ).size ===
      detail.slots.items.filter((slot) => slot.accountId !== null).length &&
    detail.slots.items.every(
      (s) =>
        s.deckRevisionId === null ||
        (s.controllerKind === "human" && s.accountId === owner),
    )
  );
}

/** Preserve labels exactly; PostgreSQL btrim removes ASCII spaces, not other whitespace. */
export const roomTeamSchema = z.strictObject({
  slotId: identifierSchema,
  teamRef: z
    .string()
    .refine(
      (text) =>
        !text.includes("\0") &&
        !/[\uD800-\uDFFF]/u.test(text) &&
        Array.from(text.replace(/^ +| +$/g, "")).length >= 1 &&
        Array.from(text.replace(/^ +| +$/g, "")).length <= 128,
    )
    .nullable(),
});
const teamAssignmentsSchema = z
  .array(roomTeamSchema)
  .max(6)
  .refine(
    (teams) => new Set(teams.map((team) => team.slotId)).size === teams.length,
  );
export const configureRoomSchema = z
  .strictObject({
    requestId: identifierSchema,
    expectedRevision: amountSchema,
    profileId: identifierSchema,
    structure: z.enum(["bo1", "bo3"]),
    teams: teamAssignmentsSchema,
  })
  .refine(
    (input) => new TextEncoder().encode(JSON.stringify(input)).length <= 32768,
  );
export type ConfigureRoom = z.infer<typeof configureRoomSchema>;
export const roomConfigurationAckSchema = z.strictObject({
  resourceId: identifierSchema,
  revision: amountSchema,
});
export const hostRoomConfigurationSchema = z
  .strictObject({
    accountId: identifierSchema,
    roomId: identifierSchema,
    revision: amountSchema,
    profileId: identifierSchema,
    structure: z.enum(["bo1", "bo3"]),
    teams: teamAssignmentsSchema,
  })
  .refine((value) =>
    value.teams.every(
      (team, index) =>
        index === 0 || team.slotId > (value.teams[index - 1]?.slotId ?? ""),
    ),
  );
export const roomConfigurationErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ room: z.literal("unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
  z.strictObject({ configuration: z.literal("unavailable") }),
  z.strictObject({
    teams: z.enum([
      "complete slot assignment required",
      "foreign slot",
      "duplicate slot",
    ]),
  }),
]);
/** Required after shape parsing; a no-op keeps the basis, a changed configuration advances once. */
export function validRoomConfigurationAck(
  command: ConfigureRoom,
  room: string,
  ack: z.infer<typeof roomConfigurationAckSchema>,
): boolean {
  return (
    ack.resourceId === room &&
    (ack.revision === command.expectedRevision ||
      BigInt(ack.revision) === BigInt(command.expectedRevision) + 1n)
  );
}
/** Read authorization is independently enforced by SQL; this binds its caller and target response. */
export function validHostRoomConfiguration(
  value: z.infer<typeof hostRoomConfigurationSchema>,
  owner: string,
  room: string,
): boolean {
  return value.accountId === owner && value.roomId === room;
}
