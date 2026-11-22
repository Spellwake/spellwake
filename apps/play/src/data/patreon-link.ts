/**
 * The one Patreon link request this device holds for an account. It is saved
 * before Patreon is opened, so the return can be matched to it, and kept
 * after linking so the link can be read again. The state it holds is private:
 * it goes to the application API and nowhere else, never into an address, a
 * log or a query key. Deleting the account erases it with the account's other
 * saved data.
 */
import { z } from "zod";
import {
  identifierSchema,
  patreonCompleteRequestSchema,
  patreonLinkRequestSchema,
  type PatreonCompleteRequest,
  type PatreonLinkRequest,
} from "@spellwake/application-contracts";
import { publicOrigin } from "../lib/public-config";
import { withAccountStorageLock } from "./account-storage";

const recordSchema = z.strictObject({
  version: z.literal(1),
  requestId: patreonLinkRequestSchema.shape.requestId,
  stateKey: patreonLinkRequestSchema.shape.stateKey,
  /** "opened" until this device has seen the link completed. */
  phase: z.enum(["opened", "linked"]),
});
export type PatreonLinkRecord = z.infer<typeof recordSchema>;

export function patreonLinkKey(apiOrigin: string, accountId: string): string {
  const account = identifierSchema.safeParse(accountId);
  if (publicOrigin(apiOrigin) !== apiOrigin || !account.success)
    throw new Error("Patreon link scope is unavailable.");
  return `spellwake.patreon-link.v1:${apiOrigin}:${account.data.toLowerCase()}`;
}

/** The saved request, or null when there is none or it cannot be trusted. */
export function readPatreonLink(
  apiOrigin: string,
  accountId: string,
  storage: Pick<Storage, "getItem"> = window.localStorage,
): PatreonLinkRecord | null {
  try {
    const raw = storage.getItem(patreonLinkKey(apiOrigin, accountId));
    if (raw === null) return null;
    const parsed = recordSchema.safeParse(JSON.parse(raw));
    return parsed.success ? parsed.data : null;
  } catch {
    return null;
  }
}

/**
 * Saves or clears the request and confirms the write by reading it back.
 * Returns false when this device would not keep it, in which case Patreon
 * must not be opened: its return could not be matched.
 */
export async function writePatreonLink(
  apiOrigin: string,
  accountId: string,
  record: PatreonLinkRecord | null,
): Promise<boolean> {
  try {
    const key = patreonLinkKey(apiOrigin, accountId);
    return await withAccountStorageLock(apiOrigin, accountId, () => {
      const storage = window.localStorage;
      if (record === null) {
        storage.removeItem(key);
        return storage.getItem(key) === null;
      }
      const raw = JSON.stringify(recordSchema.parse(record));
      storage.setItem(key, raw);
      return storage.getItem(key) === raw;
    });
  } catch {
    console.warn("patreon_link code=storage_unavailable");
    return false;
  }
}

export const flowOf = (record: PatreonLinkRecord): PatreonLinkRequest => ({
  requestId: record.requestId,
  stateKey: record.stateKey,
});

/** What came back in the address when Patreon returned the person to the app. */
export type PatreonReturn =
  | { kind: "code"; code: string; state: string }
  /** The person declined, or Patreon reported a problem. */
  | { kind: "refused" };

/** Reads a return address's query. Null when it is not a Patreon return at all. */
export function parsePatreonReturn(search: string): PatreonReturn | null {
  if (search.length > 8192) return null;
  const query = new URLSearchParams(search);
  if (query.has("error")) return { kind: "refused" };
  const code = query.get("code");
  const state = query.get("state");
  return code !== null && state !== null ? { kind: "code", code, state } : null;
}

/**
 * The completion to send for a return, or null when the return does not
 * belong to the request this device saved. A return for any other state is
 * never sent: it could be someone else's authorisation.
 */
export function completionFor(
  record: PatreonLinkRecord | null,
  returned: PatreonReturn,
): PatreonCompleteRequest | null {
  if (
    !record ||
    record.phase !== "opened" ||
    returned.kind !== "code" ||
    returned.state !== record.stateKey
  )
    return null;
  const parsed = patreonCompleteRequestSchema.safeParse({
    ...flowOf(record),
    code: returned.code,
  });
  return parsed.success ? parsed.data : null;
}
