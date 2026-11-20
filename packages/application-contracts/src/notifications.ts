/** Recipient-owned notification text; source metadata is never action authority. */
import { z } from "zod";
import { identifierSchema, amountSchema, instantSchema } from "./primitives.ts";
export const readNotificationsSchema = z
  .strictObject({
    requestId: identifierSchema,
    notificationIds: z.array(identifierSchema).min(1).max(100),
  })
  .refine(
    (input) =>
      new Set(input.notificationIds).size === input.notificationIds.length,
  );
const text = z
  .string()
  .refine((s) => !s.includes("\0") && !/[\uD800-\uDFFF]/u.test(s));
export const notificationSchema = z.strictObject({
  id: identifierSchema,
  sourceKind: text,
  sourceRef: text,
  body: text,
  readAt: instantSchema.nullable(),
});
export const notificationPageSchema = z
  .strictObject({
    accountId: identifierSchema,
    unreadCount: amountSchema,
    items: z.array(notificationSchema).max(100),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (page) =>
      page.items.every(
        (item, index) =>
          index === 0 || (page.items[index - 1]?.id ?? "") < item.id,
      ) &&
      BigInt(page.unreadCount) >=
        BigInt(page.items.filter((n) => n.readAt === null).length),
  );
export type NotificationPage = z.infer<typeof notificationPageSchema>;
/** UUID paging and unread counts are independent; bind the actual request separately. */
export function validNotificationPage(
  page: NotificationPage,
  owner: string,
  after: string | null,
  limit: number,
): boolean {
  return (
    identifierSchema.safeParse(owner).success &&
    (after === null || identifierSchema.safeParse(after).success) &&
    page.accountId === owner &&
    Number.isInteger(limit) &&
    limit >= 1 &&
    limit <= 100 &&
    page.items.length <= limit &&
    page.items.every((item) => after === null || item.id > after) &&
    page.nextCursor ===
      (page.items.length === limit ? (page.items.at(-1)?.id ?? null) : null)
  );
}
