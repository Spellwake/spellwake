import { z } from "zod";
import { identifierSchema } from "./primitives.ts";

/** Asset definition identity is independent of anyone's owned or held quantity. */
export const assetIdentitySchema = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("coin"), currencyKey: z.string().min(1) }),
  z.strictObject({ kind: z.literal("card"), printingId: identifierSchema }),
  z.strictObject({ kind: z.literal("pack"), recipeId: identifierSchema }),
]);
