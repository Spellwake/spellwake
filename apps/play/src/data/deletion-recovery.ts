/** Read-only recovery and exact-source local cleanup. Never admits a deletion. */
import { useEffect, useState } from "react";
import { useQuery, type QueryClient, type Query } from "@tanstack/react-query";
import { z } from "zod";
import {
  accountSessionSchema,
  type DeletionObservation,
} from "@spellwake/application-contracts";
import type { ApplicationServices } from "../auth/services";
import { deletionHttp } from "./deletion";
import { economyQueryFamilies } from "./economy-cache";
import {
  deletionObserver,
  deletionStatusHash,
  deletionJournalKey,
  readDeletionJournal,
  type DeletionJournal,
} from "./deletion-journal";
import {
  eraseAccountPayloads,
  removeFinalizedAccountObserver,
} from "./account-storage";

export function useDeletionObservation(
  services: ApplicationServices,
  record: DeletionJournal,
) {
  // Contains the seed; use only in component memory, never query keys or events.
  const privateBinding = JSON.stringify([
    record.apiOrigin,
    record.authOrigin,
    record.accountId,
    record.subjectId,
    record.requestId,
    record.statusSeed,
  ]);
  const [source, setSource] = useState<{
    binding: string;
    hash: string;
  } | null>(null);
  useEffect(() => {
    let alive = true;
    void deletionStatusHash(record)
      .then((hash) => {
        if (alive) setSource({ binding: privateBinding, hash });
      })
      .catch(() => {
        console.warn("deletion_client code=source_unavailable");
      });
    return () => {
      alive = false;
    };
  }, [privateBinding, record]);
  const hash = source?.binding === privateBinding ? source.hash : null;
  const query = useQuery({
    queryKey: [
      "deletion-observer",
      services.api.origin,
      services.authOrigin,
      record.accountId,
      record.subjectId,
      record.requestId,
      hash ?? "pending",
    ],
    enabled: hash !== null,
    queryFn: async ({ signal }) => {
      const observer = await deletionObserver(record);
      const response = await deletionHttp(
        (bounded) => services.api.observeDeletion(observer, bounded),
        signal,
      );
      if (response.status !== "success")
        throw new Error("Deletion progress is unavailable. Try again.");
      return { sourceHash: hash, observation: response.data };
    },
    refetchInterval: 10000,
    retry: false,
    staleTime: 0,
    gcTime: 0,
  });
  const observation =
    hash && query.isSuccess && query.data.sourceHash === hash
      ? query.data.observation
      : null;
  return { query, observation, sourceHash: hash };
}
const canonicalResponse = z.strictObject({
  status: z.literal("success"),
  data: accountSessionSchema,
});
/** Preserve another account, including canonical queries whose keys use generations. */
export function ownsDeletionQuery(
  query: Pick<Query, "queryKey" | "state">,
  record: Pick<DeletionJournal, "accountId" | "subjectId"> &
    Partial<Pick<DeletionJournal, "apiOrigin">>,
): boolean {
  if (query.queryKey[0] === "deletion-observer") return false;
  if (query.queryKey[0] === "account-session") {
    const value = canonicalResponse.safeParse(query.state.data);
    return (
      value.success &&
      value.data.data.accountId === record.accountId &&
      value.data.data.subjectId === record.subjectId
    );
  }
  if (
    typeof query.queryKey[0] === "string" &&
    [
      ...economyQueryFamilies,
      "social-directory",
      "social-threads",
      "direct-messages",
      "social-status",
    ].includes(query.queryKey[0])
  )
    return (
      record.apiOrigin !== undefined &&
      query.queryKey[1] === record.accountId &&
      query.queryKey[2] === record.apiOrigin
    );
  const ownerFamilies = new Set([
    "account-summary",
    "own-profile",
    "patreon-link",
    "deck-library",
    "owned-deck",
    "deck-formats",
    "cosmetics",
    "purchases",
    "purchase",
    "card-search",
    "printing-search",
    "deck-labels",
  ]);
  return (
    typeof query.queryKey[0] === "string" &&
    ownerFamilies.has(query.queryKey[0]) &&
    query.queryKey[1] === record.accountId
  );
}
/** Caller owns only this record lock. All HTTP/Auth work happens outside owner locks. */
export async function finishDeletionRecovery(
  services: ApplicationServices,
  queries: QueryClient,
  record: DeletionJournal,
  sourceHash: string,
  signal: AbortSignal,
): Promise<DeletionObservation> {
  const key = deletionJournalKey(record.apiOrigin, record.accountId);
  if (
    record.apiOrigin !== services.api.origin ||
    record.authOrigin !== services.authOrigin ||
    !navigator.locks
  )
    throw new Error("Deletion scope is unavailable.");
  return navigator.locks.request(key, { signal }, async () => {
    const check = () => {
      if (signal.aborted) throw new Error("Deletion recovery was interrupted.");
    };
    check();
    const storage = window.localStorage;
    const raw = storage.getItem(key);
    const current = readDeletionJournal(
      key,
      services.api.origin,
      services.authOrigin,
      storage,
    );
    if (
      !raw ||
      !current ||
      (await deletionStatusHash(current)) !== sourceHash ||
      current.subjectId !== record.subjectId
    )
      throw new Error("Deletion recovery changed. Check again.");
    check();
    // Re-observe under the record lock: cached/local flags cannot authorize erasure.
    const observer = await deletionObserver(current);
    const response = await deletionHttp(
      (bounded) => services.api.observeDeletion(observer, bounded),
      signal,
    );
    if (
      response.status !== "success" ||
      response.data.phase !== "deleted" ||
      storage.getItem(key) !== raw
    )
      throw new Error("Account deletion has not been confirmed.");
    const proof = response.data;
    check();
    await eraseAccountPayloads(services.api.origin, current.accountId, proof);
    check();
    await services.persistence.retireSubject(current.subjectId);
    check();
    const session = await services.auth.getSession();
    if (session.error || session.data.session?.user.id === current.subjectId)
      throw new Error("Deleted credentials could not be cleared. Try again.");
    check();
    const predicate = (query: Query) => ownsDeletionQuery(query, current);
    await queries.cancelQueries({ predicate });
    check();
    queries.removeQueries({ predicate });
    if (storage.getItem(key) !== raw)
      throw new Error("Deletion recovery changed. Check again.");
    await removeFinalizedAccountObserver(
      services.api.origin,
      current.accountId,
      proof,
      raw,
    );
    return proof;
  });
}
