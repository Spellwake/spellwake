import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  useRef,
  type ReactNode,
} from "react";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import type { AccountSession } from "@spellwake/application-contracts";
import {
  defaultApplicationServices,
  type ApplicationServices,
  type ServicesState,
} from "./services";

type AccountState =
  | { status: "unconfigured" }
  | { status: "loading" }
  | { status: "signed_out" }
  | { status: "rejected" }
  | { status: "unavailable" }
  | { status: "ready"; session: AccountSession };
type AccountContextValue = {
  state: AccountState;
  services: ApplicationServices | null;
  refresh: () => Promise<void>;
  isCurrentSession: (expected: AccountSession) => boolean;
};
const AccountContext = createContext<AccountContextValue | null>(null);

function AccountSessionProvider({
  services,
  children,
}: {
  services: ApplicationServices;
  children: ReactNode;
}) {
  const queries = useQueryClient();
  const authorityGeneration = useRef(0);
  const credentialPending = useRef(false);
  const [credential, setCredential] = useState<{
    initialized: boolean;
    bearer: string | null;
    generation: number;
  }>({ initialized: false, bearer: null, generation: 0 });
  useEffect(() => {
    let alive = true;
    let observed = 0;
    let installed: { bearer: string | null } | null = null;
    const apply = (bearer: string | null) => {
      if (!alive) return;
      credentialPending.current = false;
      if (installed?.bearer === bearer) {
        // The SDK repeats SIGNED_IN when another window recovers the same
        // credential. Recheck authority without unmounting unsaved drafts.
        void queries.invalidateQueries({
          queryKey: ["account-session", authorityGeneration.current],
        });
        return;
      }
      installed = { bearer };
      const generation = ++authorityGeneration.current;
      // Cancel before clearing so an old user's in-flight response cannot
      // repopulate an owner cache after a sign-out or session change.
      void queries.cancelQueries();
      queries.clear();
      setCredential({
        initialized: true,
        bearer,
        generation,
      });
    };
    const readCredential = () => {
      const attempt = ++observed;
      void services.auth
        .getSession()
        .then((result) => {
          if (alive && observed === attempt)
            apply(result.data.session?.access_token ?? null);
        })
        .catch(() => {
          if (alive && observed === attempt) apply(null);
        });
    };
    const { data } = services.auth.onAuthStateChange((_event, session) => {
      // SDK event payloads can arrive after a replacement has persisted.
      // Invalidate old continuations synchronously; re-read outside the SDK
      // callback's promise to avoid awaiting a lock it may still hold.
      if (installed && installed.bearer !== (session?.access_token ?? null))
        credentialPending.current = true;
      readCredential();
    });
    const persistenceChanged = () => {
      credentialPending.current = true;
      readCredential();
    };
    const storageChanged = (event: StorageEvent) => {
      if (
        event.key === null ||
        event.key === services.persistence.key ||
        event.key?.startsWith(
          `spellwake.auth-retired.v1:${services.persistence.key}:`,
        )
      )
        persistenceChanged();
    };
    window.addEventListener(
      services.persistence.changeEvent,
      persistenceChanged,
    );
    window.addEventListener("storage", storageChanged);
    readCredential();
    return () => {
      alive = false;
      authorityGeneration.current++;
      data.subscription.unsubscribe();
      credentialPending.current = true;
      window.removeEventListener(
        services.persistence.changeEvent,
        persistenceChanged,
      );
      window.removeEventListener("storage", storageChanged);
    };
  }, [services, queries]);
  const canonical = useQuery({
    queryKey: ["account-session", credential.generation],
    enabled: credential.initialized && credential.bearer !== null,
    queryFn: ({ signal }) =>
      services.api.session(credential.bearer ?? "", signal),
    retry: false,
    staleTime: 0,
    refetchOnWindowFocus: true,
  });
  let state: AccountState;
  if (!credential.initialized) state = { status: "loading" };
  else if (!credential.bearer) state = { status: "signed_out" };
  else if (canonical.isPending) state = { status: "loading" };
  else if (canonical.data?.status === "success")
    state = { status: "ready", session: canonical.data.data };
  else if (
    canonical.data?.status === "fail" &&
    canonical.data.data["session"] === "unauthorized"
  )
    state = { status: "rejected" };
  else state = { status: "unavailable" };
  const value: AccountContextValue = {
    state,
    services,
    refresh: async () => {
      await canonical.refetch();
    },
    // SDK events invalidate old continuations synchronously, before React's
    // next render. This fence never establishes account authority itself.
    isCurrentSession: (expected) =>
      !credentialPending.current &&
      authorityGeneration.current === credential.generation &&
      state.status === "ready" &&
      state.session.subjectId === expected.subjectId &&
      state.session.sessionId === expected.sessionId &&
      state.session.accountId === expected.accountId,
  };
  return (
    <AccountContext.Provider value={value}>{children}</AccountContext.Provider>
  );
}

/** One client/cache root serves both browser and the native WebView. */
export function AccountProvider({
  children,
  configuration,
}: {
  children: ReactNode;
  configuration?: ServicesState;
}) {
  const [queries] = useState(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: false, gcTime: 0 } },
      }),
  );
  const services = useMemo(
    () => configuration ?? defaultApplicationServices(),
    [configuration],
  );
  return (
    <QueryClientProvider client={queries}>
      {services.status === "ready" ? (
        <AccountSessionProvider services={services.services}>
          {children}
        </AccountSessionProvider>
      ) : (
        <AccountContext.Provider
          value={{
            state: { status: "unconfigured" },
            services: null,
            refresh: async () => {},
            isCurrentSession: () => false,
          }}
        >
          {children}
        </AccountContext.Provider>
      )}
    </QueryClientProvider>
  );
}

export function useAccount(): AccountContextValue {
  const value = useContext(AccountContext);
  if (!value) throw new Error("AccountProvider is required.");
  return value;
}
