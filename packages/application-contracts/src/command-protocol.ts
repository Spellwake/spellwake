/** Shared durable command state and response shape; domains supply exact acknowledgments/errors. */
import { z } from "zod";
export function makeCommandDispositionSchema<
  Errors extends z.ZodType,
  Ack extends z.ZodType,
>(errors: Errors, ack: Ack) {
  return z.discriminatedUnion("state", [
    z.strictObject({ state: z.literal("unknown"), response: z.null() }),
    z.strictObject({ state: z.literal("pending"), response: z.null() }),
    z.strictObject({
      state: z.literal("accepted"),
      response: z.strictObject({
        status: z.literal("success"),
        data: ack,
      }),
    }),
    z.strictObject({
      state: z.literal("rejected"),
      response: z.strictObject({
        status: z.literal("fail"),
        data: errors,
      }),
    }),
  ]);
}
