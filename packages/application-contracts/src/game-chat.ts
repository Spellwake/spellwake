/** Admitted human chat; public identities and messages retain their existing schemas. */
import { z } from "zod";
import {
  identifierSchema,
  publicAccountIdentitySchema,
  instantNanoseconds,
} from "./primitives.ts";
import {
  directMessageSchema,
  messageCursorSchema,
  messagePositionBefore,
  socialMessageBodySchema,
  type MessageCursor,
} from "./social.ts";
export const sendGameMessageSchema = z.strictObject({
  requestId: identifierSchema,
  body: socialMessageBodySchema,
});
export const gameChatPageSchema = z
  .strictObject({
    accountId: identifierSchema,
    matchId: identifierSchema,
    conversationId: identifierSchema,
    participants: z.array(publicAccountIdentitySchema).min(1).max(6),
    canSend: z.boolean(),
    items: z.array(directMessageSchema).max(16),
    nextCursor: messageCursorSchema.nullable(),
  })
  .refine(
    (page) =>
      page.participants.some((p) => p.id === page.accountId) &&
      page.participants.every(
        (p, i) => i === 0 || (page.participants[i - 1]?.id ?? "") < p.id,
      ) &&
      new Set(page.items.map((m) => m.id)).size === page.items.length &&
      page.items.every((m, i) => {
        const previous = page.items[i - 1];
        return (
          page.participants.some(
            (p) => p.id === m.senderId && p.username === m.author,
          ) &&
          (i === 0 ||
            (previous !== undefined && messagePositionBefore(m, previous)))
        );
      }),
  );
export type GameChatPage = z.infer<typeof gameChatPageSchema>;
/** Caller and cursor context must be checked after shape parsing. */
export function validGameChatPage(
  page: GameChatPage,
  owner: string,
  match: string,
  before: MessageCursor | null,
  limit: number,
): boolean {
  const last = page.items.at(-1);
  return (
    identifierSchema.safeParse(owner).success &&
    identifierSchema.safeParse(match).success &&
    page.accountId === owner &&
    page.matchId === match &&
    Number.isInteger(limit) &&
    limit >= 1 &&
    limit <= 16 &&
    page.items.length <= limit &&
    (before === null ||
      (messageCursorSchema.safeParse(before).success &&
        page.items.every((m) => messagePositionBefore(m, before)))) &&
    (page.items.length === limit
      ? last !== undefined &&
        page.nextCursor !== null &&
        last.id === page.nextCursor.id &&
        instantNanoseconds(last.sentAt) ===
          instantNanoseconds(page.nextCursor.sentAt)
      : page.nextCursor === null)
  );
}
