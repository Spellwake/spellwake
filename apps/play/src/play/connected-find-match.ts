import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { enqueueMatchmakingAckSchema } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { READ_ATTEMPTS } from "../collection/connected-inventory";
import { ownerBearer, useDeckLibrary } from "../data/account";
import type { PlayMode } from "../decks/deck-sources";
import {
  activeEntry,
  enqueueFailure,
  enqueueRequest,
  queueChoices,
  queueEntries,
  searchTerms,
  type FindMatchSource,
  type MatchLength,
} from "./find-match";

/** Where a found match leads: the table, or its room if the seat could not be readied. */
export type Found =
  { to: "table"; roomId: string } | { to: "room"; roomId: string };

/** Total tries at readying the seat of a found match. */
const READY_ATTEMPTS = 3;

/**
 * Finding a match for one of the account's decks. The search is a queue entry
 * the server keeps; this reads it every two seconds while it waits. A match
 * is made as a room whose seats are not ready, so the seat is readied here
 * the moment the entry is matched and the player goes straight to the table.
 */
export function useConnectedFindMatch(
  mode: PlayMode,
  deck: { id: string; format: string },
  onFound: (found: Found) => void,
): FindMatchSource {
  const { state, services } = useAccount();
  const session = state.status === "ready" ? state.session : null;
  const owner = useMemo(
    () =>
      session?.accountId ? { ...session, accountId: session.accountId } : null,
    [session],
  );
  const library = useDeckLibrary();
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = library;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);
  const saved = useMemo(
    () =>
      (library.data?.pages ?? [])
        .flatMap((page) => page.items)
        .find((item) => item.id === deck.id) ?? null,
    [library.data, deck.id],
  );

  const profiles = useQuery({
    queryKey: ["table-profiles", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    staleTime: Infinity,
    retry: READ_ATTEMPTS - 1,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to play.");
      const result = await services.api.tableProfiles(
        await ownerBearer(services, owner),
        owner.accountId,
        signal,
      );
      if (result.status !== "success") throw new Error("Rules unavailable.");
      return result.data.items;
    },
  });
  const matchmaking = useMemo(
    () => (profiles.data ?? []).filter((profile) => profile.hasMatchmaking),
    [profiles.data],
  );
  const configurations = useQueries({
    queries: matchmaking.map((profile) => ({
      queryKey: ["profile-configurations", owner?.accountId, profile.id],
      enabled: !!owner && !!services,
      staleTime: Infinity,
      retry: READ_ATTEMPTS - 1,
      queryFn: async ({ signal }: { signal: AbortSignal }) => {
        if (!owner || !services) throw new Error("Sign in to play.");
        const result = await services.api.profileConfigurations(
          await ownerBearer(services, owner),
          owner.accountId,
          profile.id,
          signal,
        );
        if (result.status !== "success") throw new Error("Rules unavailable.");
        return [profile.id, result.data.items] as const;
      },
    })),
    combine: (results) => ({
      settled: results.every((result) => !result.isPending),
      byProfile: new Map(
        results.flatMap((result) => (result.data ? [result.data] : [])),
      ),
    }),
  });
  const choices = useMemo(
    () =>
      saved && !profiles.isPending && configurations.settled
        ? queueChoices(matchmaking, configurations.byProfile, saved.format.id)
        : null,
    [saved, profiles.isPending, configurations, matchmaking],
  );

  const [phase, setPhase] = useState<FindMatchSource["phase"]>("idle");
  const [failure, setFailure] = useState<string | null>(null);
  const [search, setSearch] = useState<{
    intentId: string | null;
    since: number;
    terms: string | null;
  } | null>(null);

  // The queue is read while a search runs, and once on opening: a search
  // started earlier and still waiting is picked up where it stands.
  const queue = useQuery({
    queryKey: ["matchmaking-queue", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    refetchInterval: phase === "searching" ? 2000 : false,
    queryFn: async ({ signal }) => {
      if (!owner || !services) return [];
      const read = await services.api.queueState(
        await ownerBearer(services, owner),
        null,
        50,
        signal,
      );
      if (read.status !== "success" || !Array.isArray(read.data["items"]))
        throw new Error("Queue unavailable.");
      return queueEntries(read.data["items"]);
    },
  });
  const entry = useMemo(
    () =>
      queue.data ? activeEntry(queue.data, search?.intentId ?? null) : null,
    [queue.data, search?.intentId],
  );
  useEffect(() => {
    if (phase === "idle" && !search && entry?.state === "queued") {
      setSearch({ intentId: entry.intentId, since: Date.now(), terms: null });
      setPhase("searching");
    }
  }, [phase, search, entry]);

  // Matched: ready this seat in the match's room, then go to the table.
  const matchedRoom =
    phase === "searching" && entry && entry.state !== "queued"
      ? entry.claimedRoomId
      : null;
  const found = useRef(onFound);
  useEffect(() => {
    found.current = onFound;
  }, [onFound]);
  useEffect(() => {
    if (!matchedRoom || !owner || !services) return;
    let stopped = false;
    setPhase("found");
    void (async () => {
      let ready = false;
      for (let attempt = 0; attempt < READY_ATTEMPTS && !ready; attempt += 1) {
        try {
          const bearer = await ownerBearer(services, owner);
          const room = await services.api.roomEnrichedDetail(
            bearer,
            matchedRoom,
          );
          if (room.status !== "success") continue;
          const seat = room.data.slots.find(
            (slot) => slot.accountId === owner.accountId,
          );
          if (seat?.ready) ready = true;
          else {
            const sent = await services.api.roomControl(
              bearer,
              matchedRoom,
              "ready",
              {
                requestId: crypto.randomUUID(),
                expectedRevision: room.data.room.revision,
              },
            );
            ready = sent.status === "success";
          }
        } catch {
          ready = false;
        }
      }
      if (!stopped)
        found.current({ to: ready ? "table" : "room", roomId: matchedRoom });
    })();
    return () => {
      stopped = true;
    };
  }, [matchedRoom, owner, services]);

  const refetchQueue = queue.refetch;
  const find = useCallback(
    (length: MatchLength, ranked: boolean) => {
      if (!owner || !services || !saved || !choices) return;
      const request = saved.currentRevisionId
        ? enqueueRequest({
            requestId: crypto.randomUUID(),
            mode,
            ranked,
            deckRevisionId: saved.currentRevisionId,
            choices: choices[length],
          })
        : null;
      if (!request) {
        setFailure(
          saved.currentRevisionId
            ? "This match isn’t available for this format."
            : "Save this deck before finding a match.",
        );
        return;
      }
      setFailure(null);
      setPhase("starting");
      void (async () => {
        try {
          const sent = await services.api.enqueueMatchmaking(
            await ownerBearer(services, owner),
            request,
          );
          if (sent.status === "success") {
            const ack = enqueueMatchmakingAckSchema.safeParse(
              sent.data["ack"] ?? sent.data,
            );
            setSearch({
              intentId: ack.success ? ack.data.intentId : null,
              since: Date.now(),
              terms: searchTerms(
                deck.format,
                length,
                ranked && mode === "collection",
              ),
            });
            setPhase("searching");
            void refetchQueue();
            return;
          }
          setFailure(
            sent.status === "fail"
              ? enqueueFailure(sent.data)
              : "Couldn’t start the search.",
          );
        } catch {
          setFailure("Couldn’t start the search.");
        }
        setPhase("idle");
      })();
    },
    [owner, services, saved, choices, mode, deck.format, refetchQueue],
  );

  const cancel = useCallback(async () => {
    if (!owner || !services || !entry || entry.state !== "queued") {
      setSearch(null);
      setPhase((current) => (current === "found" ? current : "idle"));
      return;
    }
    try {
      const sent = await services.api.cancelMatchmaking(
        await ownerBearer(services, owner),
        entry.id,
        { requestId: crypto.randomUUID() },
      );
      // "Already reserved" means a match was made first: the next read finds it.
      if (sent.status === "success") {
        setSearch(null);
        setPhase("idle");
      } else setFailure("Couldn’t cancel the search.");
    } catch {
      setFailure("Couldn’t cancel the search.");
    }
    void refetchQueue();
  }, [owner, services, entry, refetchQueue]);

  return {
    lengths: choices && {
      single: choices.single.length > 0,
      three: choices.three.length > 0,
    },
    phase,
    since: search?.since ?? null,
    terms: search?.terms ?? null,
    failure,
    find,
    cancel,
  };
}
