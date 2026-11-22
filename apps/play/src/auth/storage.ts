/** Supported SDK persistence. Storage identity scopes erasure; it never authenticates. */
import type { SupportedStorage } from "@supabase/supabase-js";
import { z } from "zod";
import { identifierSchema } from "@spellwake/application-contracts";
import { publicOrigin } from "../lib/public-config";

const storedIdentity = z.object({ user: z.object({ id: identifierSchema }) });
const retiredSchema = z.strictObject({
  version: z.literal(1),
  subjectHash: z.string().regex(/^[0-9a-f]{64}$/),
});

function subjectOf(raw: string): string {
  return storedIdentity.parse(JSON.parse(raw)).user.id.toLowerCase();
}
async function hashSubject(subject: string): Promise<string> {
  const bytes = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(subject),
  );
  return Array.from(new Uint8Array(bytes), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

export type AuthPersistence = {
  storage: SupportedStorage;
  key: string;
  changeEvent: string;
  /** Caller must first establish terminal deletion through the actual observer. */
  retireSubject: (subjectId: string) => Promise<void>;
};

/** One lock covers SDK get/set/remove and conditional erasure, with no network waits. */
export function createAuthPersistence(origin: string): AuthPersistence {
  if (publicOrigin(origin) !== origin)
    throw new Error("Auth storage scope is invalid.");
  const key = `spellwake.auth.v1:${new URL(origin).host}`;
  const mutex = `spellwake.auth-storage.v1:${key}`;
  const changeEvent = `spellwake.auth-storage-changed.v1:${key}`;
  const retiredKey = (digest: string) =>
    `spellwake.auth-retired.v1:${key}:${digest}`;
  const identityKey = (candidate: string) =>
    candidate === key || candidate === `${key}-user`;
  function allowed(candidate: string) {
    if (
      identityKey(candidate) ||
      candidate === `${key}-code-verifier` ||
      candidate === `${key}-flows-code-verifier`
    )
      return;
    const prefix = `${key}-flow-`;
    if (
      candidate.startsWith(prefix) &&
      /^[A-Za-z0-9_-]{8,64}-code-verifier$/.test(candidate.slice(prefix.length))
    )
      return;
    throw new Error("Auth storage key is invalid.");
  }
  async function transition<Value>(work: () => Value): Promise<Value> {
    if (typeof navigator === "undefined" || !navigator.locks)
      throw new Error("Auth storage is unavailable.");
    try {
      return await navigator.locks.request(mutex, work);
    } catch {
      console.warn("auth_storage code=transition_unconfirmed");
      throw new Error("Auth storage could not be verified.");
    }
  }
  function closed(storage: Storage, digest: string): boolean {
    const raw = storage.getItem(retiredKey(digest));
    if (raw === null) return false;
    const record = retiredSchema.parse(JSON.parse(raw));
    if (record.subjectHash !== digest) throw new Error("Auth closure differs.");
    return true;
  }
  async function read(candidate: string): Promise<string | null> {
    allowed(candidate);
    if (!identityKey(candidate))
      return transition(() => window.localStorage.getItem(candidate));
    // Hash outside the lock. Re-read under it before returning; a replacement
    // during hashing must restart rather than return the old subject's record.
    for (let attempt = 0; attempt < 8; attempt++) {
      const raw = await transition(() =>
        window.localStorage.getItem(candidate),
      );
      if (raw === null) return null;
      let digest: string;
      try {
        digest = await hashSubject(subjectOf(raw));
      } catch {
        console.warn("auth_storage code=identity_unavailable");
        throw new Error("Auth storage could not be verified.");
      }
      const result = await transition(() => {
        const storage = window.localStorage;
        if (storage.getItem(candidate) !== raw)
          return { changed: true } as const;
        if (closed(storage, digest))
          throw new Error("Auth subject is retired.");
        return { changed: false, raw } as const;
      });
      if (!result.changed) return result.raw;
    }
    console.warn("auth_storage code=read_contended");
    throw new Error("Auth storage changed; retry.");
  }
  const storage: SupportedStorage = {
    getItem: read,
    async setItem(candidate, raw) {
      allowed(candidate);
      let digest: string | null = null;
      try {
        if (identityKey(candidate)) digest = await hashSubject(subjectOf(raw));
      } catch {
        console.warn("auth_storage code=identity_unavailable");
        throw new Error("Auth storage could not be verified.");
      }
      await transition(() => {
        const local = window.localStorage;
        if (digest !== null && closed(local, digest))
          throw new Error("Auth subject is retired.");
        local.setItem(candidate, raw);
        if (local.getItem(candidate) !== raw)
          throw new Error("Auth save was not confirmed.");
      });
    },
    async removeItem(candidate) {
      allowed(candidate);
      await transition(() => {
        const local = window.localStorage;
        local.removeItem(candidate);
        if (local.getItem(candidate) !== null)
          throw new Error("Auth removal was not confirmed.");
      });
    },
  };
  return {
    storage,
    key,
    changeEvent,
    async retireSubject(subjectId) {
      try {
        const subject = identifierSchema.parse(subjectId).toLowerCase();
        const digest = await hashSubject(subject);
        await transition(() => {
          const local = window.localStorage;
          // Validate an existing fence rather than overwriting corrupt state.
          closed(local, digest);
          const raw = JSON.stringify(
            retiredSchema.parse({ version: 1, subjectHash: digest }),
          );
          local.setItem(retiredKey(digest), raw);
          if (local.getItem(retiredKey(digest)) !== raw)
            throw new Error("Auth closure was not confirmed.");
          for (const candidate of [key, `${key}-user`]) {
            const current = local.getItem(candidate);
            if (current === null || subjectOf(current) !== subject) continue;
            local.removeItem(candidate);
            if (local.getItem(candidate) !== null)
              throw new Error("Auth erasure was not confirmed.");
          }
          // PKCE verifier/index records belong to ongoing sign-in flows, not
          // this retired subject. Conditional erasure never removes them.
        });
      } catch {
        console.warn("auth_storage code=erasure_unconfirmed");
        throw new Error("Deleted account credentials could not be removed.");
      } finally {
        // Public consumers re-read the SDK. Do not fabricate SIGNED_OUT or
        // overwrite the subject/token of a concurrently persisted replacement.
        if (typeof window !== "undefined")
          window.dispatchEvent(new Event(changeEvent));
      }
    },
  };
}
