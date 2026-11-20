/** Canonical accepted cosmetic selections; IDs alone do not establish entitlement. */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";
export const appearanceSchema = z.strictObject({
  backId: identifierSchema,
  sleeveId: identifierSchema,
  diceId: identifierSchema,
  playmatId: identifierSchema.nullable(),
});
export type AppearanceSelection = z.infer<typeof appearanceSchema>;
