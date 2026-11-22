import { useMemo } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { z } from "zod";
import {
  identifierSchema,
  importDeckAckSchema,
  importDeckDispositionSchema,
  importDeckErrorsSchema,
  importDeckSchema,
  type ImportDeck,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "./account";
import { withAccountStorageLock } from "./account-storage";
import { createIntentJournal } from "./intent-journal";
import { recoverIntent, type RecoverableAction } from "./intent-recovery";

const source = {
  version: z.literal(1),
  apiOrigin: z.string(),
  accountId: identifierSchema,
  command: importDeckSchema,
};
const journalSchema = z.discriminatedUnion("state", [
  z.strictObject({ ...source, state: z.literal("unresolved") }),
  z.strictObject({
    ...source,
    state: z.literal("accepted"),
    ack: importDeckAckSchema,
  }),
  z.strictObject({
    ...source,
    state: z.literal("rejected"),
    errors: importDeckErrorsSchema,
  }),
]);
export type DeckImportJournal = z.infer<typeof journalSchema>;
type Errors = z.infer<typeof importDeckErrorsSchema>;
type Draft = Omit<ImportDeck, "requestId">;

/**
 * The one import this device has sent for an account, kept until it is
 * settled. It sits in the deck-command family, so deleting the account
 * erases it with the rest.
 */
export function createDeckImportJournal(
  apiOrigin: string,
  accountId: string,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  const key = `spellwake.deck-command.v1:${apiOrigin}:${accountId}:import`;
  return createIntentJournal<ImportDeck, Errors>(
    key,
    apiOrigin,
    accountId,
    journalSchema.refine(
      (value) => value.apiOrigin === apiOrigin && value.accountId === accountId,
    ),
    importDeckSchema,
    importDeckDispositionSchema,
    (command) => command.requestId,
    getStorage,
  );
}

/** Sends one deck import, settling an earlier unanswered one first. */
export function useDeckImportCommand() {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  const queries = useQueryClient();
  const store = useMemo(
    () =>
      createDeckImportJournal(
        services?.api.origin ?? "unconfigured",
        owner?.accountId ?? "",
        () => window.localStorage,
      ),
    [services, owner?.accountId],
  );
  const journal = useAtomValue(store.stateAtom);
  const act = useSetAtom(store.actionAtom);
  const run = useMutation({
    mutationFn: async (
      input: RecoverableAction<Draft, ImportDeck, Errors>,
    ): Promise<DeckImportJournal> => {
      if (!owner || !services) throw new Error("Sign in to import a deck.");
      if (input.type === "submit" && owner.state !== "active")
        throw new Error("An active account is required.");
      const result = await recoverIntent(input, {
        key: store.key,
        act: (action) =>
          withAccountStorageLock(services.api.origin, owner.accountId, () =>
            act(action),
          ),
        build: (draft) =>
          importDeckSchema.parse({ ...draft, requestId: crypto.randomUUID() }),
        send: async (command) =>
          services.api.importDeck(await ownerBearer(services, owner), command),
        lookup: async (command) =>
          services.api.importDeckDisposition(
            await ownerBearer(services, owner),
            command,
          ),
        refreshCurrent: async () => {
          await ownerBearer(services, owner);
          if (!isCurrentSession(owner))
            throw new Error("Your session changed. Reopen Import deck.");
          void queries.invalidateQueries({
            queryKey: ["deck-library", owner.accountId],
          });
        },
      });
      return journalSchema.parse(result);
    },
  });
  return { journal, run };
}
