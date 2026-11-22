import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import type { AccountSession } from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import type { ApplicationServices } from "../auth/services";

/** SDK identity only partitions requests; the API still authenticates the bearer. */
export async function ownerBearer(
  services: { auth: Pick<ApplicationServices["auth"], "getSession"> },
  expected: AccountSession,
): Promise<string> {
  const current = await services.auth.getSession();
  if (
    current.error ||
    !current.data.session ||
    current.data.session.user.id !== expected.subjectId
  )
    throw new Error("Sign in again to load your account.");
  return current.data.session.access_token;
}
export function useAccountSummary() {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  return useQuery({
    queryKey: ["account-summary", owner?.accountId, owner?.sessionId],
    enabled: owner !== null && services !== null,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to load your account.");
      const result = await services.api.account(
        await ownerBearer(services, owner),
        signal,
      );
      if (
        result.status !== "success" ||
        result.data.accountId !== owner.accountId
      )
        throw new Error("Your account could not be loaded.");
      return result.data;
    },
    refetchInterval: 30000,
    refetchOnWindowFocus: true,
  });
}
/** The account's own profile art and whether it is a current supporter. */
export function useOwnProfile() {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  return useQuery({
    queryKey: ["own-profile", owner?.accountId, owner?.sessionId],
    enabled: owner !== null && services !== null,
    queryFn: async ({ signal }) => {
      if (!owner?.accountId || !services)
        throw new Error("Sign in to load your profile.");
      const result = await services.api.ownProfile(
        await ownerBearer(services, owner),
        owner.accountId,
        signal,
      );
      if (result.status !== "success")
        throw new Error("Your profile could not be loaded.");
      return result.data;
    },
    refetchOnWindowFocus: true,
  });
}
export function useDeckLibrary() {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  return useInfiniteQuery({
    queryKey: ["deck-library", owner?.accountId, owner?.sessionId],
    enabled: owner !== null && services !== null,
    initialPageParam: null as string | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services) throw new Error("Sign in to load your decks.");
      const result = await services.api.decks(
        await ownerBearer(services, owner),
        pageParam,
        signal,
      );
      if (result.status !== "success")
        throw new Error("Your decks could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
  });
}
