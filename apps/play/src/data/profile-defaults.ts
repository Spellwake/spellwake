import { useMemo } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { z } from "zod";
import {
  identifierSchema,
  commandAckSchema,
  changeProfileDefaultsSchema,
  profileDefaultsErrorsSchema,
  profileDefaultsDispositionSchema,
  type ChangeProfileDefaults,
} from "@spellwake/application-contracts";
import { useAccount } from "../auth/AccountProvider";
import { ownerBearer } from "./account";
import { createIntentJournal } from "./intent-journal";
import { recoverIntent, type RecoverableAction } from "./intent-recovery";
import { withAccountStorageLock } from "./account-storage";

const source = {
  version: z.literal(1),
  apiOrigin: z.string(),
  accountId: identifierSchema,
  command: changeProfileDefaultsSchema,
};
const journalSchema = z.discriminatedUnion("state", [
  z.strictObject({ ...source, state: z.literal("unresolved") }),
  z.strictObject({
    ...source,
    state: z.literal("accepted"),
    ack: commandAckSchema,
  }),
  z.strictObject({
    ...source,
    state: z.literal("rejected"),
    errors: profileDefaultsErrorsSchema,
  }),
]);
export type ProfileDefaultsJournal = z.infer<typeof journalSchema>;
type Errors = z.infer<typeof profileDefaultsErrorsSchema>;
type Draft = Omit<ChangeProfileDefaults, "requestId">;

/** Account defaults retain a distinct key, envelope, contract and command scope. */
export function createProfileDefaultsJournal(
  apiOrigin: string,
  accountId: string,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  const key = `spellwake.profile-defaults.v1:${apiOrigin}:${accountId}`;
  const schema = journalSchema.refine(
    (value) => value.apiOrigin === apiOrigin && value.accountId === accountId,
  );
  return createIntentJournal<ChangeProfileDefaults, Errors>(
    key,
    apiOrigin,
    accountId,
    schema,
    changeProfileDefaultsSchema,
    profileDefaultsDispositionSchema,
    (command) => command.requestId,
    getStorage,
  );
}

export function useProfileDefaultsCommand() {
  const { state, services, isCurrentSession } = useAccount();
  const owner =
    state.status === "ready" && state.session.accountId ? state.session : null;
  const queries = useQueryClient();
  const store = useMemo(
    () =>
      createProfileDefaultsJournal(
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
      input: RecoverableAction<Draft, ChangeProfileDefaults, Errors>,
    ): Promise<ProfileDefaultsJournal> => {
      if (!owner || !services)
        throw new Error("Sign in to save your defaults.");
      if (input.type === "submit" && owner.state !== "active")
        throw new Error("An active account is required.");
      return recoverIntent(input, {
        key: store.key,
        act: (action) =>
          withAccountStorageLock(services.api.origin, owner.accountId, () =>
            act(action),
          ),
        build: (draft) =>
          changeProfileDefaultsSchema.parse({
            ...draft,
            requestId: crypto.randomUUID(),
          }),
        send: async (command) =>
          services.api.changeProfileDefaults(
            await ownerBearer(services, owner),
            command,
          ),
        lookup: async (command) =>
          services.api.profileDefaultsDisposition(
            await ownerBearer(services, owner),
            command,
          ),
        refreshCurrent: async () => {
          await ownerBearer(services, owner);
          if (!isCurrentSession(owner))
            throw new Error(
              "Your session changed. Reopen Cosmetics to continue.",
            );
          void queries.invalidateQueries({
            queryKey: ["account-summary", owner.accountId],
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
