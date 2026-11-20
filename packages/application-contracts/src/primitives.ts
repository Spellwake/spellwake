import { z } from "zod";

export const usernameSchema = z
  .string()
  .regex(
    /^[A-Za-z0-9_]{3,24}$/,
    "Use 3 to 24 letters, numbers or underscores.",
  );

export const identifierSchema = z
  .string()
  .regex(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/, {
    abort: true,
  })
  .refine((value) => value !== "00000000-0000-0000-0000-000000000000");
// Stop malformed decimals before downstream balance refinements use BigInt.
export const amountSchema = z
  .string()
  .regex(/^(0|[1-9][0-9]{0,77})$/, { abort: true });
export const positiveAmountSchema = z.string().regex(/^[1-9][0-9]{0,77}$/);
export const instantSchema = z.iso.datetime({ offset: true });

/** Public history uses one permanent anonymous label after retirement. */
export const publicAccountIdentitySchema = z
  .strictObject({
    id: identifierSchema,
    username: z.string(),
    isDeleted: z.boolean(),
  })
  .refine((value) =>
    value.isDeleted
      ? value.username === "Deleted player"
      : usernameSchema.safeParse(value.username).success,
  );

/** Date supplies whole UTC seconds only; fractional ordering stays exact to nanoseconds. */
export function instantNanoseconds(value: string): bigint | null {
  const millis = Date.parse(value);
  const fraction = /\.(\d+)/.exec(value)?.[1] ?? "";
  if (!Number.isFinite(millis) || fraction.length > 9) return null;
  return (
    BigInt(Math.floor(millis / 1000)) * 1_000_000_000n +
    BigInt(fraction.padEnd(9, "0"))
  );
}

/** Canonical calendar day; malformed dates never become normalized rollover days. */
export const daySchema = z
  .string()
  .regex(/^\d{4}-\d{2}-\d{2}$/)
  .refine((value) => {
    const instant = new Date(`${value}T00:00:00Z`);
    return (
      Number.isFinite(instant.getTime()) &&
      instant.toISOString().startsWith(value)
    );
  });
