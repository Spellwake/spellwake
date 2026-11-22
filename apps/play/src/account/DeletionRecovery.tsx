import { useEffect, type ReactElement } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";
import { refreshDeletionRegistry, useDeletionRegistry } from "../data/deletion";
import {
  deletionJournalKey,
  type DeletionJournal,
} from "../data/deletion-journal";
import {
  finishDeletionRecovery,
  useDeletionObservation,
} from "../data/deletion-recovery";

/**
 * Follows one requested deletion to its end. It only ever reads: it never
 * sends a deletion request, including one that was prepared and not answered.
 * When the server confirms the account is deleted, this device's copy of the
 * account and its sign-in are cleared, which shows sign-in.
 */
function DeletionWatch({
  services,
  record,
}: {
  services: ApplicationServices;
  record: DeletionJournal;
}): null {
  const queries = useQueryClient();
  const { query, observation, sourceHash } = useDeletionObservation(
    services,
    record,
  );
  const isDeleted = observation?.phase === "deleted";
  // Each fresh confirmation is another chance to finish a cleanup that failed.
  const confirmedAt = query.dataUpdatedAt;
  useEffect(() => {
    if (!isDeleted || !sourceHash) return;
    const attempt = new AbortController();
    void finishDeletionRecovery(
      services,
      queries,
      record,
      sourceHash,
      attempt.signal,
    )
      .then(() =>
        refreshDeletionRegistry(
          deletionJournalKey(record.apiOrigin, record.accountId),
        ),
      )
      .catch(() => {
        // Nothing is shown: the next confirmation tries again. The reason may
        // name private storage, so it is not logged.
        if (!attempt.signal.aborted)
          console.warn("deletion_client code=cleanup_unconfirmed");
      });
    return () => attempt.abort();
    // The record is re-read from storage on every scan, so it is compared by
    // the request it holds, not by object identity.
  }, [isDeleted, sourceHash, confirmedAt, services, queries, record.requestId]);
  return null;
}

function Recovery({
  services,
}: {
  services: ApplicationServices;
}): ReactElement {
  const registry = useDeletionRegistry(services);
  return (
    <>
      {registry.records.map((record) => (
        <DeletionWatch
          key={`${record.accountId}:${record.requestId}`}
          services={services}
          record={record}
        />
      ))}
    </>
  );
}

/**
 * Mounted once, outside every screen, so a requested deletion is followed
 * whether its owner is signed in, signed out, or someone else is using this
 * device. It draws nothing.
 */
export function DeletionRecovery(): ReactElement | null {
  const { services } = useAccount();
  return services ? <Recovery services={services} /> : null;
}
