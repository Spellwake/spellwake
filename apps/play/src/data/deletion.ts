/** Explicit confirmation admits deletion; recovery observes only. */
import { useMemo } from "react";
import { atom, useAtomValue, useSetAtom } from "jotai";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { AccountSession } from "@spellwake/application-contracts";
import { identifierSchema } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer } from "./account";
import { withAccountStorageLock } from "./account-storage";
import { subscribeDurableStorage } from "./durable-cell";
import {
  createDeletionJournal,
  deletionJournalKey,
  deletionJournalChanged,
  deletionStatusHash,
  newDeletionSeed,
  readDeletionJournal,
  type DeletionJournal,
} from "./deletion-journal";

export type DeletionRegistry = {
  records: DeletionJournal[];
  unavailable: string[];
  readable: boolean;
};
function scan(services: ApplicationServices): DeletionRegistry {
  try {
    const prefix = `spellwake.deletion.v1:${services.api.origin}:`;
    const result: DeletionRegistry = {
      records: [],
      unavailable: [],
      readable: true,
    };
    for (const key of Object.keys(window.localStorage).filter((key) =>
      key.startsWith(prefix),
    )) {
      const accountId = key.slice(prefix.length);
      if (!identifierSchema.safeParse(accountId).success) continue;
      try {
        const value = readDeletionJournal(
          key,
          services.api.origin,
          services.authOrigin,
          window.localStorage,
        );
        if (value) result.records.push(value);
      } catch {
        result.unavailable.push(accountId);
      }
    }
    return result;
  } catch {
    return { records: [], unavailable: [], readable: false };
  }
}
/** No persisted index: scope/key validation precedes any observer work. */
export function useDeletionRegistry(services: ApplicationServices) {
  const registry = useMemo(() => {
    const value = atom(scan(services));
    value.onMount = (set) => {
      let subscribed: (() => void)[] = [];
      const refresh = () => {
        subscribed.forEach((stop) => stop());
        const next = scan(services);
        set(next);
        subscribed = [
          ...next.records.map((record) => record.accountId),
          ...next.unavailable,
        ].map((id) =>
          subscribeDurableStorage(
            deletionJournalKey(services.api.origin, id),
            refresh,
          ),
        );
      };
      // New slots have no per-record subscription yet.
      const storage = (event: StorageEvent) => {
        if (
          event.key === null ||
          event.key.startsWith(`spellwake.deletion.v1:${services.api.origin}:`)
        )
          refresh();
      };
      const local = () => refresh();
      window.addEventListener("storage", storage);
      window.addEventListener("spellwake.deletion-records-changed.v1", local);
      refresh();
      return () => {
        subscribed.forEach((stop) => stop());
        window.removeEventListener("storage", storage);
        window.removeEventListener(
          "spellwake.deletion-records-changed.v1",
          local,
        );
      };
    };
    return value;
  }, [services]);
  return useAtomValue(registry);
}
export function refreshDeletionRegistry(key: string): void {
  deletionJournalChanged(key);
  window.dispatchEvent(new Event("spellwake.deletion-records-changed.v1"));
}
/** A local timeout bounds HTTP work, not settlement or deletion finality. */
export async function deletionHttp<Value>(
  work: (signal: AbortSignal) => Promise<Value>,
  parent?: AbortSignal,
): Promise<Value> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  if (parent?.aborted) controller.abort();
  parent?.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(abort, 15000);
  try {
    return await work(controller.signal);
  } finally {
    clearTimeout(timer);
    parent?.removeEventListener("abort", abort);
  }
}

/** Only call mutate after explicit confirmation in the current account's dialog. */
export function useConfirmedDeletion(
  owner: AccountSession & { accountId: string },
  services: ApplicationServices,
) {
  const { isCurrentSession } = useAccount();
  const queries = useQueryClient();
  const store = useMemo(
    () =>
      createDeletionJournal(
        {
          apiOrigin: services.api.origin,
          authOrigin: services.authOrigin,
          accountId: owner.accountId,
          subjectId: owner.subjectId,
        },
        () => window.localStorage,
      ),
    [services, owner.accountId, owner.subjectId],
  );
  const saved = useAtomValue(store.stateAtom);
  const act = useSetAtom(store.actionAtom);
  const mutation = useMutation({
    mutationFn: async () => {
      const current = () => {
        if (
          !isCurrentSession(owner) ||
          !["active", "deletion_pending"].includes(owner.state)
        )
          throw new Error("Your session changed. Reopen account settings.");
      };
      current();
      if (!navigator.locks)
        throw new Error("Deletion recovery is unavailable in this browser.");
      return navigator.locks.request(store.key, async () => {
        current();
        const record = await withAccountStorageLock(
          services.api.origin,
          owner.accountId,
          () =>
            act({
              type: "prepare",
              requestId: crypto.randomUUID(),
              statusSeed: newDeletionSeed(),
            }),
        );
        if (!record)
          throw new Error("Couldn’t save the deletion request. Try again.");
        refreshDeletionRegistry(store.key);
        const statusKeyHash = await deletionStatusHash(record);
        current();
        const bearer = await ownerBearer(services, owner);
        current();
        const response = await deletionHttp((signal) =>
          services.api.requestDeletion(
            bearer,
            { requestId: record.requestId, statusKeyHash },
            signal,
          ),
        );
        if (
          response.status !== "success" ||
          response.data.resourceId !== owner.accountId
        )
          throw new Error(
            "Deletion hasn’t been confirmed. Check progress or try again.",
          );
        const accepted = await withAccountStorageLock(
          services.api.origin,
          owner.accountId,
          () =>
            act({
              type: "accept",
              expected: record,
              resourceId: response.data.resourceId,
            }),
        );
        if (!accepted)
          throw new Error("The request was sent. Check deletion progress.");
        refreshDeletionRegistry(store.key);
        await queries.invalidateQueries({
          queryKey: [
            "deletion-observer",
            services.api.origin,
            services.authOrigin,
            owner.accountId,
          ],
        });
        if (isCurrentSession(owner))
          await queries.invalidateQueries({ queryKey: ["account-session"] });
        return accepted;
      });
    },
  });
  return { saved, mutation };
}
