import { useMemo } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import {
  useMutation,
  useQuery,
  useInfiniteQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  deckCommandSchema,
  identifierSchema,
  type CreateDeck,
  type SaveDeck,
  type ChangeDeckSettings,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "./account";
import { recoverIntent } from "./intent-recovery";
import { withAccountStorageLock } from "./account-storage";
import {
  createDeckJournalStore,
  type DeckJournal,
  type DeckJournalSlot,
} from "./deck-journal";

type DeckDraft =
  | { kind: "create"; command: Omit<CreateDeck, "requestId"> }
  | { kind: "save"; command: Omit<SaveDeck, "requestId"> }
  | { kind: "settings"; command: Omit<ChangeDeckSettings, "requestId"> };
type CommandAction =
  | { type: "submit"; expected: DeckJournal | null; draft: DeckDraft }
  | { type: "recover"; expected: DeckJournal };

/** Exact-body recovery survives sessions; all journal transitions share one exclusive lock. */
export function useDeckCommand(slot: DeckJournalSlot) {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  const queries = useQueryClient();
  const deckId = slot.kind === "create" ? null : slot.deckId;
  const kind = slot.kind;
  const store = useMemo(
    () =>
      createDeckJournalStore(
        services?.api.origin ?? "unconfigured",
        owner?.accountId ?? "",
        deckId === null || kind === "create"
          ? { kind: "create" }
          : { kind, deckId },
        () => window.localStorage,
      ),
    [services, owner?.accountId, deckId, kind],
  );
  const journal = useAtomValue(store.stateAtom);
  const act = useSetAtom(store.actionAtom);
  const run = useMutation({
    mutationFn: async (input: CommandAction): Promise<DeckJournal> => {
      if (!owner || !services) throw new Error("Sign in to save your deck.");
      if (input.type === "submit" && owner.state !== "active")
        throw new Error("An active account is required.");
      return recoverIntent(input, {
        key: store.key,
        act: (action) =>
          withAccountStorageLock(services.api.origin, owner.accountId, () =>
            act(action),
          ),
        build: (draft) =>
          deckCommandSchema.parse({
            ...draft,
            command: { ...draft.command, requestId: crypto.randomUUID() },
          }),
        send: async (command) => {
          const bearer = await ownerBearer(services, owner);
          return command.kind === "create"
            ? services.api.createDeck(bearer, command.command)
            : command.kind === "save"
              ? services.api.saveDeck(bearer, command.command)
              : services.api.changeDeckSettings(bearer, command.command);
        },
        lookup: async (command) =>
          services.api.deckCommandDisposition(
            await ownerBearer(services, owner),
            command,
          ),
        refreshCurrent: async () => {
          await ownerBearer(services, owner);
          if (!isCurrentSession(owner))
            throw new Error(
              "Your session changed. Reopen the deck to continue.",
            );
          void queries.invalidateQueries({
            queryKey: ["deck-library", owner.accountId],
          });
          void queries.invalidateQueries({
            queryKey: ["owned-deck", owner.accountId],
          });
        },
      });
    },
  });
  return {
    journal,
    run,
    isCurrent: () => owner !== null && isCurrentSession(owner),
  };
}

export function useOwnedDeck(deckId: string) {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  return useQuery({
    queryKey: ["owned-deck", owner?.accountId, owner?.sessionId, deckId],
    enabled:
      !!owner && !!services && identifierSchema.safeParse(deckId).success,
    queryFn: async ({ signal }) => {
      if (!owner || !services) throw new Error("Sign in to open your deck.");
      const result = await services.api.deck(
        await ownerBearer(services, owner),
        deckId,
        signal,
      );
      if (result.status !== "success")
        throw new Error("This deck could not be loaded.");
      return result.data;
    },
  });
}

export function useDeckFormats() {
  const { state, services } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  return useInfiniteQuery({
    queryKey: ["deck-formats", owner?.accountId, owner?.sessionId],
    enabled: !!owner && !!services,
    initialPageParam: null as string | null,
    queryFn: async ({ signal, pageParam }) => {
      if (!owner || !services) throw new Error("Sign in to choose a format.");
      const result = await services.api.formats(
        await ownerBearer(services, owner),
        pageParam,
        signal,
      );
      if (result.status !== "success")
        throw new Error("Formats could not be loaded.");
      return result.data;
    },
    getNextPageParam: (page) => page.nextCursor ?? undefined,
  });
}
