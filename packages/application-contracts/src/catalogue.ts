import { z } from "zod";
import { identifierSchema } from "./primitives.ts";

function label(maximum: number) {
  return z
    .string()
    .max(maximum * 2)
    .refine(
      (value) =>
        Array.from(value).length >= 1 &&
        Array.from(value).length <= maximum &&
        !value.includes("\0"),
    );
}
export const cardSummarySchema = z.strictObject({
  id: identifierSchema,
  name: label(1024),
  oracleRef: z.string().regex(/^[\x21-\x7e]{1,128}$/),
});
export const setSummarySchema = z.strictObject({
  id: identifierSchema,
  name: label(1024),
  code: z
    .string()
    .regex(/^[A-Za-z0-9]{1,16}$/)
    .nullable(),
});
export const printingSummarySchema = z.strictObject({
  id: identifierSchema,
  definitionId: identifierSchema,
  externalPrintingRef: z.string().regex(/^[\x21-\x7e]{1,128}$/),
  rarity: z.string().regex(/^[\x21-\x7e]{1,64}$/),
  finish: z.string().regex(/^[\x21-\x7e]{1,64}$/),
  collectorNumber: label(128).nullable(),
  language: z
    .string()
    .regex(/^[A-Za-z0-9_-]{1,16}$/)
    .nullable(),
  set: setSummarySchema,
});
function ordered(items: readonly { id: string }[]) {
  return items.every(
    (item, index) => index === 0 || (items[index - 1]?.id ?? "") < item.id,
  );
}
function cataloguePage<T extends z.ZodType<{ id: string }>>(item: T) {
  return z
    .strictObject({
      items: z.array(item).max(100),
      nextCursor: identifierSchema.nullable(),
    })
    .refine(
      (page) =>
        ordered(page.items) &&
        (page.nextCursor === null || page.nextCursor === page.items.at(-1)?.id),
      "Invalid catalogue cursor.",
    );
}
export const setPageSchema = cataloguePage(setSummarySchema);
export const cardPageSchema = cataloguePage(cardSummarySchema);
export const printingPageSchema = cataloguePage(printingSummarySchema);
export const catalogueQuerySchema = z.strictObject({
  query: z
    .string()
    .max(2048)
    .refine(
      (value) => Array.from(value).length <= 1024 && !value.includes("\0"),
    ),
  exact: z.boolean(),
});
export const namedPrintingQuerySchema = z.strictObject({
  name: label(1024),
  setCode: printingSummarySchema.shape.set.shape.code,
  collectorNumber: printingSummarySchema.shape.collectorNumber,
});
const lookupIds = z
  .array(identifierSchema)
  .max(100)
  .refine(
    (ids) => new Set(ids).size === ids.length,
    "Duplicate catalogue identities.",
  );
export const catalogueLookupSchema = z.strictObject({
  definitionIds: lookupIds,
  printingIds: lookupIds,
});
export const catalogueLabelsSchema = z
  .strictObject({
    cards: z.array(cardSummarySchema).max(100),
    printings: z.array(printingSummarySchema).max(100),
  })
  .refine(
    (labels) => ordered(labels.cards) && ordered(labels.printings),
    "Invalid catalogue ordering.",
  );
export type CardSummary = z.infer<typeof cardSummarySchema>;
export type PrintingSummary = z.infer<typeof printingSummarySchema>;
export type CatalogueLookup = z.infer<typeof catalogueLookupSchema>;
export type CatalogueLabels = z.infer<typeof catalogueLabelsSchema>;
