/** Native-facing wire scalars. These schemas do not activate a player endpoint,
 * resolve handles, authorize a seat or qualify a native projection.
 */
import { z } from "zod";
import { identifierSchema } from "./primitives.ts";

/** Canonical signed arbitrary precision rules value, distinct from coin Amount. */
export const rulesIntegerSchema = z.string().refine(
  (value) => {
    try {
      return BigInt(value).toString() === value;
    } catch {
      return false;
    }
  },
  { abort: true },
);
/** Arbitrary precision count used by game rules, without an economy digit cap. */
export const rulesCountSchema = rulesIntegerSchema.refine(
  (value) => !value.startsWith("-"),
  { abort: true },
);
/** Checked native counter; an application host fence is a separate authority. */
export const nativeRevisionSchema = rulesCountSchema.refine(
  (value) => BigInt(value) <= 18_446_744_073_709_551_615n,
);
/** Complete public basis. Its context ID resolves to trusted pinned native inputs. */
export const playerBasisSchema = z.strictObject({
  gameId: identifierSchema,
  contextId: identifierSchema,
  epoch: nativeRevisionSchema,
  revision: nativeRevisionSchema,
});
/** Original issued token syntax only, never proof of native identity or authority. */
export const playerHandleSchema = z.string().refine(
  (value) =>
    value.length > 0 &&
    value.length <= 256 &&
    [...value].every((char) => {
      const code = char.charCodeAt(0);
      return code >= 33 && code <= 126;
    }),
);
export type PlayerBasis = z.infer<typeof playerBasisSchema>;

/** Bounded display fields, distinct from opaque handles and arithmetic values. */
export function playerDisplayTextSchema(maximumBytes: number) {
  return z
    .string()
    .refine(
      (value) =>
        new TextEncoder().encode(value).length <= maximumBytes &&
        !value.includes("\0") &&
        !/[\uD800-\uDFFF]/u.test(value),
    );
}

/** Exact native English-label representation, without Oracle/vocabulary membership claims. */
export const playerEnglishLabelSchema = z
  .string()
  .refine(
    (value) =>
      value.length > 0 &&
      !/^\p{White_Space}|\p{White_Space}$/u.test(value) &&
      !/\p{Cc}/u.test(value) &&
      !/[\uD800-\uDFFF]/u.test(value),
  );

/** Exact validated wire equality: object key order is irrelevant; array order is retained. */
export function samePlayerWireValue(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b))
    return (
      a.length === b.length && a.every((v, i) => samePlayerWireValue(v, b[i]))
    );
  if (
    typeof a !== "object" ||
    a === null ||
    typeof b !== "object" ||
    b === null ||
    Array.isArray(a) ||
    Array.isArray(b)
  )
    return false;
  const left = Object.entries(a);
  const right = Object.entries(b);
  return (
    left.length === right.length &&
    left.every(([k, v]) =>
      right.some(([key, value]) => key === k && samePlayerWireValue(v, value)),
    )
  );
}

/** Only revisions in the same game/context/epoch are comparable here.
 * Cross-epoch lineage remains an independently verified source responsibility.
 */
export function playerBasisRegresses(
  previous: PlayerBasis,
  next: PlayerBasis,
): boolean {
  return (
    previous.gameId === next.gameId &&
    previous.contextId === next.contextId &&
    previous.epoch === next.epoch &&
    BigInt(previous.revision) > BigInt(next.revision)
  );
}

export const playerCardTypeSchema = z.enum([
  "planeswalker",
  "land",
  "artifact",
  "creature",
  "enchantment",
  "kindred",
  "battle",
  "instant",
  "sorcery",
]);
