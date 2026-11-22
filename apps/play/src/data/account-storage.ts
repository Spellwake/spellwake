import { z } from "zod";
import {
  identifierSchema,
  deletionObservationSchema,
  type DeletionObservation,
} from "@spellwake/application-contracts";
import { publicOrigin } from "./application-api";
import { notifyDurableStorage } from "./durable-cell";

const closedSchema = z.strictObject({
  version: z.literal(1),
  apiOrigin: z.string(),
  accountId: identifierSchema,
  retirementOperationId: identifierSchema,
  authCleanupReceiptId: identifierSchema,
});

function scope(origin: string, accountId: string | null) {
  const actor = identifierSchema.safeParse(accountId);
  if (!actor.success || publicOrigin(origin) !== origin)
    throw new Error("Account storage scope is unavailable.");
  return { origin, accountId: actor.data.toLowerCase() };
}
/** One account/API boundary, distinct from any individual journal or product lock. */
export function accountStorageLockKey(
  origin: string,
  accountId: string | null,
): string {
  const selected = scope(origin, accountId);
  return `spellwake.account-storage.v1:${selected.origin}:${selected.accountId}`;
}

/** Minimal permanent fence, separate from recovery payloads and the observer secret. */
export function accountClosedStorageKey(
  origin: string,
  accountId: string,
): string {
  const selected = scope(origin, accountId);
  return `spellwake.account-closed.v1:${selected.origin}:${selected.accountId}`;
}

function closedRecord(storage: Storage, origin: string, accountId: string) {
  const raw = storage.getItem(accountClosedStorageKey(origin, accountId));
  if (raw === null) return null;
  const selected = scope(origin, accountId);
  const value = closedSchema.parse(JSON.parse(raw));
  if (value.apiOrigin !== origin || value.accountId !== selected.accountId)
    throw new Error("Account storage could not be verified.");
  return value;
}

async function accountTransition<Value>(
  origin: string,
  accountId: string | null,
  work: () => Value & (Value extends PromiseLike<unknown> ? never : unknown),
): Promise<Value> {
  if (typeof navigator === "undefined" || !navigator.locks)
    throw new Error("Account storage is unavailable in this browser.");
  const key = accountStorageLockKey(origin, accountId);
  return navigator.locks.request(key, () => {
    const result = work();
    if (
      result !== null &&
      (typeof result === "object" || typeof result === "function") &&
      "then" in result &&
      typeof result.then === "function"
    ) {
      console.error("account_storage code=async_transition");
      throw new Error("The saved attempt could not be updated.");
    }
    return result;
  });
}

/** Hold record -> account ordering for synchronous storage transitions only, never external I/O. */
export async function withAccountStorageLock<Value>(
  origin: string,
  accountId: string | null,
  work: () => Value & (Value extends PromiseLike<unknown> ? never : unknown),
): Promise<Value> {
  const selected = scope(origin, accountId);
  return accountTransition(origin, accountId, () => {
    try {
      if (closedRecord(window.localStorage, origin, selected.accountId))
        throw new Error("This account's saved data has been closed.");
    } catch {
      console.warn("account_storage code=scope_closed_or_unavailable");
      throw new Error("This account's saved data is unavailable.");
    }
    return work();
  });
}

/** Exact current payload families; never match a substring or another API/account. */
export function isAccountPayloadKey(
  key: string,
  origin: string,
  accountId: string,
): boolean {
  const selected = scope(origin, accountId);
  const suffix = `${origin}:${selected.accountId}`;
  if (key === `spellwake.profile-defaults.v1:${suffix}`) return true;
  // The Patreon link request this device holds, with its private state.
  if (key === `spellwake.patreon-link.v1:${suffix}`) return true;
  const social = `spellwake.social-command.v1:${suffix}:`;
  if (key.startsWith(social)) {
    const [kind, target, extra] = key.slice(social.length).split(":");
    return (
      extra === undefined &&
      ["friendship", "block", "open_dm", "send_dm", "read_dm"].includes(
        kind ?? "",
      ) &&
      identifierSchema.safeParse(target).success
    );
  }
  const economy = `spellwake.economy-command.v1:${suffix}:`;
  if (key.startsWith(economy)) {
    const [kind, target, extra] = key.slice(economy.length).split(":");
    return (
      extra === undefined &&
      [
        "trade_create",
        "trade_response",
        "listing_create",
        "listing_cancel",
        "listing_buy",
        "pack_buy",
        "pack_open",
      ].includes(kind ?? "") &&
      identifierSchema.safeParse(target).success
    );
  }
  // Lobby actions: creating a room, then each action on a room or AI seat.
  const room = `spellwake.room-command.v1:${suffix}:`;
  if (key.startsWith(room)) {
    const [kind, target, extra] = key.slice(room.length).split(":");
    if (kind === "create") return target === undefined;
    return (
      extra === undefined &&
      [
        "select_deck",
        "leave",
        "ready",
        "unready",
        "force_start",
        "add_ai",
        "bootstrap_ai",
        "remove_ai",
        "configure",
      ].includes(kind ?? "") &&
      identifierSchema.safeParse(target).success
    );
  }
  const draft = `spellwake.social-draft.v1:${suffix}:`;
  if (key.startsWith(draft))
    return identifierSchema.safeParse(key.slice(draft.length)).success;
  const checkout = `spellwake.checkout.v1:${suffix}:`;
  if (key.startsWith(checkout))
    return identifierSchema.safeParse(key.slice(checkout.length)).success;
  const deck = `spellwake.deck-command.v1:${suffix}:`;
  if (!key.startsWith(deck)) return false;
  const slot = key.slice(deck.length);
  if (slot === "create" || slot === "import") return true;
  for (const kind of ["save:", "settings:"])
    if (slot.startsWith(kind))
      return identifierSchema.safeParse(slot.slice(kind.length)).success;
  return false;
}

/**
 * Close and erase only this verified account's payloads under its short lock.
 * Failure preserves the permanent fence and observer for retry, never reports
 * success. This does not erase SDK credentials, React drafts/caches or backups.
 */
export async function eraseAccountPayloads(
  origin: string,
  accountId: string,
  observation: DeletionObservation,
): Promise<void> {
  const selected = scope(origin, accountId);
  const proof = deletionObservationSchema.parse(observation);
  if (proof.phase !== "deleted" || proof.accountId !== selected.accountId)
    throw new Error("Account deletion has not been confirmed.");
  const changed: string[] = [];
  try {
    await accountTransition(origin, accountId, () => {
      try {
        const storage = window.localStorage;
        const next = closedSchema.parse({
          version: 1,
          apiOrigin: origin,
          accountId: selected.accountId,
          retirementOperationId: proof.retirementOperationId,
          authCleanupReceiptId: proof.authCleanupReceiptId,
        });
        const prior = closedRecord(storage, origin, selected.accountId);
        if (prior && JSON.stringify(prior) !== JSON.stringify(next))
          throw new Error("Account deletion proof differs.");
        const raw = JSON.stringify(next);
        const key = accountClosedStorageKey(origin, selected.accountId);
        storage.setItem(key, raw);
        if (storage.getItem(key) !== raw)
          throw new Error("Account closure was not confirmed.");
        // Storage exposes enumerable own keys: take a snapshot, avoiding numeric
        // index shifts caused by another account removing unrelated keys.
        const keys = Object.keys(storage).filter((candidate) =>
          isAccountPayloadKey(candidate, origin, selected.accountId),
        );
        changed.push(...keys);
        for (const candidate of keys) {
          storage.removeItem(candidate);
          if (storage.getItem(candidate) !== null)
            throw new Error("Account payload removal was not confirmed.");
        }
        if (
          Object.keys(storage).some((candidate) =>
            isAccountPayloadKey(candidate, origin, selected.accountId),
          ) ||
          storage.getItem(key) !== raw
        )
          throw new Error("Account erasure was not confirmed.");
      } catch {
        console.warn("account_storage code=erasure_unconfirmed");
        throw new Error("Account data removal could not be confirmed.");
      }
    });
  } finally {
    notifyDurableStorage(changed);
  }
}

/**
 * Final observer removal only, after SDK/public-session cleanup. The caller
 * holds its record lock; this takes only the short owner lock and cannot open
 * a fence or execute arbitrary writes/network work.
 */
export async function removeFinalizedAccountObserver(
  origin: string,
  accountId: string,
  observation: DeletionObservation,
  expectedRaw: string,
): Promise<void> {
  const selected = scope(origin, accountId);
  const proof = deletionObservationSchema.parse(observation);
  if (proof.phase !== "deleted" || proof.accountId !== selected.accountId)
    throw new Error("Account deletion has not been confirmed.");
  const observerKey = `spellwake.deletion.v1:${origin}:${selected.accountId}`;
  try {
    await accountTransition(origin, accountId, () => {
      const storage = window.localStorage;
      const closed = closedRecord(storage, origin, accountId);
      if (
        !closed ||
        closed.retirementOperationId !== proof.retirementOperationId ||
        closed.authCleanupReceiptId !== proof.authCleanupReceiptId ||
        Object.keys(storage).some((key) =>
          isAccountPayloadKey(key, origin, accountId),
        )
      )
        throw new Error("Account erasure has not been confirmed.");
      const raw = JSON.stringify(closed);
      const current = storage.getItem(observerKey);
      if (
        storage.getItem(accountClosedStorageKey(origin, accountId)) !== raw ||
        (current !== null && current !== expectedRaw)
      )
        throw new Error("Final cleanup scope changed.");
      storage.removeItem(observerKey);
      if (
        storage.getItem(observerKey) !== null ||
        storage.getItem(accountClosedStorageKey(origin, accountId)) !== raw ||
        Object.keys(storage).some((key) =>
          isAccountPayloadKey(key, origin, accountId),
        )
      )
        throw new Error("Final cleanup was not confirmed.");
    });
  } finally {
    notifyDurableStorage([observerKey]);
  }
}
