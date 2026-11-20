/** Qualified virtual pack products and current sequential pricing, without private evidence or reservations. */
import { z } from "zod";
import {
  amountSchema,
  positiveAmountSchema,
  identifierSchema,
} from "./primitives.ts";
export const PACK_PRODUCT_PAGE_LIMIT = 32;
export const packQuantitySchema = z
  .string()
  .regex(/^[1-9][0-9]{0,3}$/)
  .refine((value) => Number(value) <= 1000);
const label = z
  .string()
  .min(1)
  .refine(
    (value) =>
      Array.from(value).length <= 1024 &&
      Array.from(value).every((char) => {
        const code = char.charCodeAt(0);
        return code > 31 && (code < 127 || code > 159);
      }),
  );
export const packProductSchema = z.strictObject({
  id: identifierSchema,
  productId: identifierSchema,
  setId: identifierSchema,
  name: label,
  setName: label,
  revisionRef: label,
});
export const packProductPageSchema = z
  .strictObject({
    accountId: identifierSchema,
    items: z.array(packProductSchema).max(PACK_PRODUCT_PAGE_LIMIT),
    nextCursor: identifierSchema.nullable(),
  })
  .refine(
    (value) =>
      new Set(value.items.map((item) => item.productId)).size ===
        value.items.length &&
      value.items.every(
        (item, index) =>
          index === 0 || (value.items[index - 1]?.id ?? "") < item.id,
      ) &&
      (value.nextCursor === null ||
        value.items.at(-1)?.id === value.nextCursor),
  );
export const packQuoteSchema = z
  .strictObject({
    recipeId: identifierSchema,
    productId: identifierSchema,
    policyId: identifierSchema,
    revision: amountSchema,
    remaining: amountSchema,
    unitPrice: positiveAmountSchema,
    quantity: packQuantitySchema,
    total: positiveAmountSchema.nullable(),
    purpose: z.enum(["purchase", "valuation_only"]),
  })
  .refine((value) => {
    if (
      !amountSchema.safeParse(value.remaining).success ||
      !packQuantitySchema.safeParse(value.quantity).success
    )
      return false;
    const available = BigInt(value.remaining) >= BigInt(value.quantity);
    return value.purpose === "purchase"
      ? available && value.total !== null
      : !available && value.total === null;
  });
export const ownedPackQuoteSchema = z.strictObject({
  accountId: identifierSchema,
  quote: packQuoteSchema,
});
export type PackProduct = z.infer<typeof packProductSchema>;
export type OwnedPackQuote = z.infer<typeof ownedPackQuoteSchema>;
