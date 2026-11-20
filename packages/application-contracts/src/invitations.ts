/** Raw capabilities are delivered intentionally; never log these requests, tokens or receipts. */
import { z } from "zod";
import { identifierSchema, amountSchema } from "./primitives.ts";
import { validProgressionPage } from "./progression.ts";
/** 32 decoded bytes with canonical unpadded base64url, including zero unused pad bits. */
export const invitationTokenSchema = z
  .string()
  .regex(/^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/);
export const issueInvitationSchema = z.strictObject({
  requestId: identifierSchema,
  expectedRevision: amountSchema,
});
export const joinRoomSchema = z.strictObject({
  requestId: identifierSchema,
  token: invitationTokenSchema.nullable(),
});
export const revokeInvitationSchema = z.strictObject({
  requestId: identifierSchema,
});
export const issuedInvitationSchema = z.strictObject({
  resourceId: identifierSchema,
  token: invitationTokenSchema,
});
export const joinRoomAckSchema = z.strictObject({
  resourceId: identifierSchema,
  slotId: identifierSchema,
});
export const issueInvitationErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ room: z.literal("unavailable") }),
  z.strictObject({ revision: z.literal("conflict") }),
]);
export const joinRoomErrorsSchema = z.union([
  z.strictObject({ room: z.literal("unavailable") }),
  z.strictObject({ invitation: z.literal("unavailable") }),
  z.strictObject({ profile: z.literal("unqualified") }),
  z.strictObject({ capacity: z.literal("full") }),
]);
export const revokeInvitationErrorsSchema = z.union([
  z.strictObject({ account: z.literal("unavailable") }),
  z.strictObject({ invitation: z.literal("unavailable") }),
]);
export const invitationSummarySchema = z.strictObject({
  id: identifierSchema,
  roomId: identifierSchema,
  state: z.enum(["active", "canceled"]),
});
export const invitationPageSchema = z.strictObject({
  accountId: identifierSchema,
  items: z.array(invitationSummarySchema).max(100),
  nextCursor: identifierSchema.nullable(),
});
/** Parse the shape first, then bind current caller, original filter and page context. */
export function validInvitationPage(
  page: z.infer<typeof invitationPageSchema>,
  owner: string,
  room: string | null,
  after: string | null,
  limit: number,
): boolean {
  return (
    validProgressionPage(page, owner, after, limit) &&
    page.items.every((item) => room === null || item.roomId === room)
  );
}
/** An accepted receipt does not promise continuing membership; always query current room state. */
export function validJoinRoomAck(
  ack: z.infer<typeof joinRoomAckSchema>,
  room: string,
): boolean {
  return ack.resourceId === room;
}
