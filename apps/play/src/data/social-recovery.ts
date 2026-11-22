/** Explicit user actions only; loading a saved attempt never sends it. */
import type {
  AccountSession,
  SocialCommandDisposition,
} from "@spellwake/application-contracts";
import type { ApplicationServices } from "../auth/services";
import { ownerBearer } from "./account";
import { withAccountStorageLock } from "./account-storage";
import { recoverIntent, type RecoverableAction } from "./intent-recovery";
import type { IntentJournalAction, IntentJournal } from "./intent-journal";
import {
  createSocialJournal,
  type ScopedSocialIntent,
  type SocialIntent,
} from "./social-journal";

type Errors = Extract<
  SocialCommandDisposition,
  { state: "rejected" }
>["response"]["data"];
export type SocialJournal = IntentJournal<ScopedSocialIntent, Errors>;
type Owner = Exclude<AccountSession, { state: "needs_registration" }>;

/** Keep record -> short owner-lock ordering; all SDK/API work runs outside owner locks. */
export async function recoverSocialIntent(
  input: RecoverableAction<SocialIntent, ScopedSocialIntent, Errors>,
  options: {
    journal: ReturnType<typeof createSocialJournal>;
    owner: Owner;
    services: Pick<ApplicationServices, "api"> & {
      auth: Pick<ApplicationServices["auth"], "getSession">;
    };
    act: (
      action: IntentJournalAction<ScopedSocialIntent, Errors>,
    ) => SocialJournal | null;
    isCurrent: () => boolean;
    refresh: () => void | Promise<void>;
  },
): Promise<SocialJournal> {
  const { journal, owner, services } = options;
  if (
    journal.origin !== services.api.origin ||
    journal.accountId !== owner.accountId ||
    journal.subjectId !== owner.subjectId ||
    !options.isCurrent()
  )
    throw new Error("Your account changed. Reopen Messages.");
  if (
    input.type === "submit" &&
    input.draft.kind === "social" &&
    owner.state !== "active"
  )
    throw new Error("This account cannot start another social action.");
  const bearer = async () => {
    if (!options.isCurrent())
      throw new Error("Your account changed. Reopen Messages.");
    const token = await ownerBearer(services, owner);
    if (!options.isCurrent())
      throw new Error("Your account changed. Reopen Messages.");
    return token;
  };
  return recoverIntent(input, {
    key: journal.key,
    build: (intent) => ({ subjectId: owner.subjectId, intent }),
    act: (action) =>
      withAccountStorageLock(journal.origin, journal.accountId, () =>
        options.act(action),
      ),
    send: async (command) => {
      if (command.subjectId !== owner.subjectId)
        throw new Error("Saved action belongs to another identity.");
      const intent = command.intent;
      if (intent.kind === "social" && owner.state !== "active")
        throw new Error("This account cannot start another social action.");
      const token = await bearer();
      return intent.kind === "social"
        ? services.api.socialCommand(token, intent.input)
        : services.api.markDirectRead(token, intent.input);
    },
    lookup: async (command) => {
      if (command.subjectId !== owner.subjectId)
        throw new Error("Saved action belongs to another identity.");
      const token = await bearer();
      return command.intent.kind === "social"
        ? services.api.socialCommandDisposition(token, command.intent.input)
        : services.api.directReadDisposition(token, command.intent.input);
    },
    refreshCurrent: async () => {
      await bearer();
      await options.refresh();
      if (!options.isCurrent())
        throw new Error("Your account changed. Reopen Messages.");
    },
  });
}
