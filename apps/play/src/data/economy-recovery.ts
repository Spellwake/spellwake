/** Explicit recovery reuses the original immutable request; reload alone performs no I/O. */
import type {
  AccountSession,
  EconomyCommand,
  EconomyCommandDisposition,
} from "@spellwake/application-contracts";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer } from "./account";
import { withAccountStorageLock } from "./account-storage";
import { recoverIntent, type RecoverableAction } from "./intent-recovery";
import type { IntentJournalAction, IntentJournal } from "./intent-journal";
import {
  createEconomyJournal,
  type ScopedEconomyIntent,
} from "./economy-journal";

type Errors = Extract<
  EconomyCommandDisposition,
  { state: "rejected" }
>["response"]["data"];
export type EconomyJournal = IntentJournal<ScopedEconomyIntent, Errors>;
type Owner = Exclude<AccountSession, { state: "needs_registration" }>;

/** SDK/network waits remain outside short owner locks; account/session closure fences every write. */
export async function recoverEconomyIntent(
  input: RecoverableAction<EconomyCommand, ScopedEconomyIntent, Errors>,
  options: {
    journal: ReturnType<typeof createEconomyJournal>;
    owner: Owner;
    services: Pick<ApplicationServices, "api"> & {
      auth: Pick<ApplicationServices["auth"], "getSession">;
    };
    act: (
      action: IntentJournalAction<ScopedEconomyIntent, Errors>,
    ) => EconomyJournal | null;
    isCurrent: () => boolean;
    refresh: () => void | Promise<void>;
  },
): Promise<EconomyJournal> {
  const { journal, owner, services } = options;
  const changed = () =>
    new Error("Your account changed. Reopen the transaction.");
  if (
    journal.origin !== services.api.origin ||
    journal.accountId !== owner.accountId ||
    journal.subjectId !== owner.subjectId ||
    !options.isCurrent()
  )
    throw changed();
  const bearer = async () => {
    if (!options.isCurrent()) throw changed();
    const token = await ownerBearer(services, owner);
    if (!options.isCurrent()) throw changed();
    return token;
  };
  const maySend = () => {
    if (owner.state !== "active")
      throw new Error("This account cannot start another economic action.");
  };
  if (input.type === "submit") maySend();
  return recoverIntent(input, {
    key: journal.key,
    build: (intent) => ({ subjectId: owner.subjectId, intent }),
    act: (action) =>
      withAccountStorageLock(journal.origin, journal.accountId, () => {
        if (!options.isCurrent()) throw changed();
        return options.act(action);
      }),
    send: async (command) => {
      if (command.subjectId !== owner.subjectId) throw changed();
      maySend();
      const token = await bearer();
      return services.api.economyCommand(token, command.intent);
    },
    lookup: async (command) => {
      if (command.subjectId !== owner.subjectId) throw changed();
      return services.api.economyCommandDisposition(
        await bearer(),
        command.intent,
      );
    },
    refreshCurrent: async () => {
      await bearer();
      await options.refresh();
      if (!options.isCurrent()) throw changed();
    },
  });
}
