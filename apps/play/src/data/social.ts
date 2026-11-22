import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import {
  socialDirectoryQuerySchema,
  type MessageCursor,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { readForOwner } from "./owner-read";

/** Full unread count and exact selected identity; discovery list pages never define the total. */
export function useSocialStatus(personId: string | null, enabled: boolean) {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  return useQuery({
    queryKey: [
      "social-status",
      owner?.accountId,
      services?.api.origin,
      owner?.subjectId,
      owner?.sessionId,
      personId,
    ],
    enabled: enabled && owner !== null && services !== null,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to load Messages.");
      const result = await readForOwner(
        { owner, services, isCurrent: () => isCurrentSession(owner) },
        signal,
        (bearer) =>
          services.api.socialStatus(bearer, owner.accountId, personId, signal),
      );
      if (result.status !== "success")
        throw new Error("Message status could not be loaded.");
      return result.data;
    },
    refetchInterval: enabled ? 15000 : false,
    refetchOnWindowFocus: true,
  });
}

/** Discovery and retained threads share the canonical paged people projection. */
export function useSocialPeople(
  kind: "directory" | "threads",
  query: string,
  enabled: boolean,
) {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  return useInfiniteQuery({
    queryKey: [
      kind === "directory" ? "social-directory" : "social-threads",
      owner?.accountId,
      services?.api.origin,
      owner?.subjectId,
      owner?.sessionId,
      kind === "directory" ? query : null,
    ],
    enabled:
      enabled &&
      owner !== null &&
      services !== null &&
      (kind === "threads" ||
        socialDirectoryQuerySchema.safeParse(query).success),
    initialPageParam: null as string | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services) throw new Error("Sign in to load Messages.");
      const result = await readForOwner(
        { owner, services, isCurrent: () => isCurrentSession(owner) },
        signal,
        (bearer) =>
          kind === "directory"
            ? services.api.socialDirectory(
                bearer,
                owner.accountId,
                query,
                pageParam,
                signal,
              )
            : services.api.socialThreads(
                bearer,
                owner.accountId,
                pageParam,
                signal,
              ),
      );
      if (result.status !== "success")
        throw new Error("People could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    refetchInterval: enabled && kind === "threads" ? 15000 : false,
    refetchOnWindowFocus: true,
  });
}

/** History stays readable when contact permission expires; sends use separate authority. */
export function useDirectMessages(
  conversationId: string | null,
  enabled: boolean,
) {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.state !== "needs_registration"
      ? state.session
      : null;
  return useInfiniteQuery({
    queryKey: [
      "direct-messages",
      owner?.accountId,
      services?.api.origin,
      owner?.subjectId,
      owner?.sessionId,
      conversationId,
    ],
    enabled:
      enabled && owner !== null && services !== null && conversationId !== null,
    initialPageParam: null as MessageCursor | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services || !conversationId)
        throw new Error("Choose a conversation.");
      const result = await readForOwner(
        { owner, services, isCurrent: () => isCurrentSession(owner) },
        signal,
        (bearer) =>
          services.api.directMessages(
            bearer,
            owner.accountId,
            conversationId,
            pageParam,
            signal,
          ),
      );
      if (result.status !== "success")
        throw new Error("This conversation could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    refetchInterval: enabled ? 10000 : false,
    refetchOnWindowFocus: true,
  });
}
