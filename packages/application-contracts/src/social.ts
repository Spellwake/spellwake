/** Viewer-scoped social data. No Auth/provider identity or invented presence. */
import { z } from "zod";
import {
  amountSchema,
  identifierSchema,
  instantSchema,
  instantNanoseconds,
  usernameSchema,
  publicAccountIdentitySchema,
} from "./primitives.ts";

export const DIRECT_MESSAGE_PAGE_LIMIT = 16;

export const socialMessageBodySchema = z
  .string()
  .refine(
    (body) =>
      body.trim().length > 0 &&
      Array.from(body).length <= 4000 &&
      !body.includes("\0") &&
      !/[\uD800-\uDFFF]/u.test(body),
    "Message must contain 1 to 4000 characters.",
  );
export const socialDirectoryQuerySchema = z.string().refine((query) => {
  const length = Array.from(query.replace(/^ +| +$/g, "")).length;
  return (
    !query.includes("\0") &&
    !/[\uD800-\uDFFF]/u.test(query) &&
    length >= 1 &&
    length <= 64
  );
});

export const socialPersonSchema = publicAccountIdentitySchema
  .safeExtend({
    relationship: z.enum([
      "none",
      "friend",
      "incoming_request",
      "outgoing_request",
    ]),
    friendshipId: identifierSchema.nullable(),
    isBlocked: z.boolean(),
    canContact: z.boolean(),
    conversationId: identifierSchema.nullable(),
    unreadMessages: amountSchema,
    supporter: z.boolean(),
  })
  .refine(
    (person) =>
      (person.relationship === "none") === (person.friendshipId === null) &&
      !(person.isBlocked && person.canContact) &&
      (person.conversationId !== null || person.unreadMessages === "0") &&
      (person.isDeleted
        ? person.relationship === "none" &&
          !person.canContact &&
          !person.supporter
        : true),
  );
export type SocialPersonSummary = z.infer<typeof socialPersonSchema>;

export const socialStatusSchema = z
  .strictObject({
    accountId: identifierSchema,
    unreadMessages: amountSchema,
    personId: identifierSchema.nullable(),
    person: socialPersonSchema.nullable(),
  })
  .refine(
    (status) =>
      status.person === null ||
      (status.person.id === status.personId &&
        status.person.id !== status.accountId &&
        amountSchema.safeParse(status.person.unreadMessages).success &&
        amountSchema.safeParse(status.unreadMessages).success &&
        BigInt(status.person.unreadMessages) <= BigInt(status.unreadMessages)),
  );
export type SocialStatus = z.infer<typeof socialStatusSchema>;

export const socialPeoplePageSchema = z
  .strictObject({
    accountId: identifierSchema,
    items: z.array(socialPersonSchema).max(100),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (page) =>
      page.items.every(
        (person, index) =>
          person.id !== page.accountId &&
          (index === 0 || (page.items[index - 1]?.id ?? "") < person.id),
      ) &&
      (page.nextCursor === null || page.items.at(-1)?.id === page.nextCursor),
  );

const messageInstantSchema = instantSchema.refine(
  (value) => instantNanoseconds(value) !== null,
);
export const messageCursorSchema = z.strictObject({
  sentAt: messageInstantSchema,
  id: identifierSchema,
});
export type MessageCursor = z.infer<typeof messageCursorSchema>;

/** Malformed times return false rather than throw from an untrusted-data refinement. */
export function messagePositionBefore(
  left: MessageCursor,
  right: MessageCursor,
): boolean {
  const a = instantNanoseconds(left.sentAt);
  const b = instantNanoseconds(right.sentAt);
  return a !== null && b !== null && (a < b || (a === b && left.id < right.id));
}

export const directMessageSchema = z.strictObject({
  id: identifierSchema,
  senderId: identifierSchema,
  author: z.union([usernameSchema, z.literal("Deleted player")]),
  body: socialMessageBodySchema,
  sentAt: messageInstantSchema,
});
export const directMessagePageSchema = z
  .strictObject({
    accountId: identifierSchema,
    conversationId: identifierSchema,
    person: socialPersonSchema,
    items: z.array(directMessageSchema).max(DIRECT_MESSAGE_PAGE_LIMIT),
    nextCursor: messageCursorSchema.nullable(),
  })
  .refine((page) => {
    const ids = new Set<string>();
    const last = page.items.at(-1);
    return (
      page.accountId !== page.person.id &&
      page.person.conversationId === page.conversationId &&
      page.items.every((message, index) => {
        if (ids.has(message.id)) return false;
        ids.add(message.id);
        const previous = page.items[index - 1];
        return (
          (message.senderId === page.person.id
            ? message.author === page.person.username
            : message.senderId === page.accountId &&
              usernameSchema.safeParse(message.author).success) &&
          (index === 0 ||
            (previous !== undefined &&
              messagePositionBefore(message, previous)))
        );
      }) &&
      (page.nextCursor === null ||
        (last !== undefined &&
          last.id === page.nextCursor.id &&
          instantNanoseconds(last.sentAt) !== null &&
          instantNanoseconds(last.sentAt) ===
            instantNanoseconds(page.nextCursor.sentAt)))
    );
  });
export type DirectMessagePage = z.infer<typeof directMessagePageSchema>;
