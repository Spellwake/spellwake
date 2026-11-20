/** Private OAuth flow contracts; no second Spellwake login or payment/grant proof. */
import { z } from "zod";
import {
  identifierSchema,
  instantSchema,
  instantNanoseconds,
} from "./primitives.ts";
export const patreonStateKeySchema = z
  .string()
  .regex(/^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/);
export const patreonLinkRequestSchema = z.strictObject({
  requestId: identifierSchema,
  stateKey: patreonStateKeySchema,
});
export const patreonCompleteRequestSchema = patreonLinkRequestSchema.extend({
  code: z
    .string()
    .min(16)
    .max(4096)
    .regex(/^[\x21-\x7e]+$/),
});
const linkInstant = instantSchema.refine((value) => {
  const nanos = instantNanoseconds(value),
    year = new Date(value).getUTCFullYear();
  return (
    /^[0-9]{4}-/.test(value) &&
    year >= 1 &&
    year <= 9999 &&
    nanos !== null &&
    nanos % 1000n === 0n
  );
});
export const patreonLinkStatusSchema = z
  .strictObject({
    accountId: identifierSchema,
    requestId: identifierSchema,
    phase: z.enum([
      "unknown",
      "issued",
      "claimed",
      "linked",
      "expired",
      "superseded",
    ]),
    expiresAt: linkInstant.nullable(),
    associationId: identifierSchema.nullable(),
  })
  .refine(
    (value) =>
      (value.phase === "unknown") === (value.expiresAt === null) &&
      (value.phase === "linked") === (value.associationId !== null),
  );
/** Only the exact official authorization endpoint and fixed identity-only query can be opened. */
export const patreonAuthorizationUrlSchema = z
  .string()
  .max(8192)
  .refine((value) => {
    try {
      const url = new URL(value),
        keys = [...url.searchParams.keys()].sort();
      const rawRedirect = url.searchParams.get("redirect_uri") ?? "";
      const redirect = new URL(rawRedirect);
      return (
        url.origin === "https://www.patreon.com" &&
        url.pathname === "/oauth2/authorize" &&
        !url.username &&
        !url.password &&
        !url.hash &&
        keys.join(",") === "client_id,redirect_uri,response_type,scope,state" &&
        url.searchParams.get("response_type") === "code" &&
        url.searchParams.get("scope") === "identity" &&
        /^[A-Za-z0-9_-]{1,128}$/.test(
          url.searchParams.get("client_id") ?? "",
        ) &&
        patreonStateKeySchema.safeParse(url.searchParams.get("state"))
          .success &&
        new TextEncoder().encode(rawRedirect).byteLength <= 2048 &&
        !Array.from(rawRedirect).some((character) => {
          const code = character.charCodeAt(0);
          return code === 32 || (code >= 9 && code <= 13);
        }) &&
        redirect.protocol === "https:" &&
        !!redirect.hostname &&
        !redirect.username &&
        !redirect.password &&
        !redirect.search &&
        !redirect.hash
      );
    } catch {
      return false;
    }
  });
export const patreonLinkStartSchema = z.strictObject({
  accountId: identifierSchema,
  requestId: identifierSchema,
  expiresAt: linkInstant,
  authorizationUrl: patreonAuthorizationUrlSchema,
});
export type PatreonLinkRequest = z.infer<typeof patreonLinkRequestSchema>;
export type PatreonCompleteRequest = z.infer<
  typeof patreonCompleteRequestSchema
>;
export type PatreonLinkStatus = z.infer<typeof patreonLinkStatusSchema>;
export type PatreonLinkStart = z.infer<typeof patreonLinkStartSchema>;
