/** Subject-bound social attempts; no record is permission for automatic sending. */
import { z } from "zod";
import { atom } from "jotai";
import {
  identifierSchema,
  socialCommandSchema,
  socialCommandErrorsSchema,
  socialCommandDispositionSchema,
  markDirectReadSchema,
  commandAckSchema,
} from "@spellwake/application-contracts";
import { publicOrigin } from "./application-api";
import { createIntentJournal } from "./intent-journal";
import { createDurableCell } from "./durable-cell";
import { withAccountStorageLock } from "./account-storage";

export const socialIntentSchema = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("social"), input: socialCommandSchema }),
  z.strictObject({ kind: z.literal("read"), input: markDirectReadSchema }),
]);
export type SocialIntent = z.infer<typeof socialIntentSchema>;
const scopedSchema = z.strictObject({
  subjectId: identifierSchema,
  intent: socialIntentSchema,
});
export type ScopedSocialIntent = z.infer<typeof scopedSchema>;
export const socialSlotSchema = z.strictObject({
  kind: z.enum(["friendship", "block", "open_dm", "send_dm", "read_dm"]),
  targetId: identifierSchema,
});
export type SocialSlot = z.infer<typeof socialSlotSchema>;
export function socialIntentSlot(intent: SocialIntent): SocialSlot {
  if (intent.kind === "read")
    return { kind: "read_dm", targetId: intent.input.conversationId };
  const input = intent.input;
  return {
    kind: input.kind,
    targetId:
      input.kind === "send_dm"
        ? input.command.conversationId
        : input.command.otherAccountId,
  };
}
export function socialIntentRequestId(command: ScopedSocialIntent): string {
  return command.intent.kind === "read"
    ? command.intent.input.requestId
    : command.intent.input.command.requestId;
}

/** Uses the shared durable transition machine with strict API/account/subject/slot binding. */
export function createSocialJournal(
  origin: string,
  accountId: string,
  subjectId: string,
  slotInput: SocialSlot,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  if (publicOrigin(origin) !== origin)
    throw new Error("Social recovery scope is unavailable.");
  identifierSchema.parse(accountId);
  identifierSchema.parse(subjectId);
  const slot = socialSlotSchema.parse(slotInput);
  const key = `spellwake.social-command.v1:${origin}:${accountId}:${slot.kind}:${slot.targetId}`;
  const commandSchema = scopedSchema.refine(
    (command) =>
      command.subjectId === subjectId &&
      socialIntentSlot(command.intent).kind === slot.kind &&
      socialIntentSlot(command.intent).targetId === slot.targetId,
  );
  const source = {
    version: z.literal(1),
    apiOrigin: z.literal(origin),
    accountId: z.literal(accountId),
    command: commandSchema,
  };
  const schema = z
    .discriminatedUnion("state", [
      z.strictObject({ ...source, state: z.literal("unresolved") }),
      z.strictObject({
        ...source,
        state: z.literal("accepted"),
        ack: commandAckSchema,
      }),
      z.strictObject({
        ...source,
        state: z.literal("rejected"),
        errors: socialCommandErrorsSchema,
      }),
    ])
    .refine((entry) => {
      if (entry.state === "unresolved") return true;
      if (entry.state === "rejected")
        return (
          entry.command.intent.kind !== "read" || "conversation" in entry.errors
        );
      const intent = entry.command.intent;
      return intent.kind === "read"
        ? entry.ack.resourceId === intent.input.conversationId
        : intent.input.kind !== "block" ||
            entry.ack.resourceId === intent.input.command.otherAccountId;
    });
  return {
    ...createIntentJournal<
      ScopedSocialIntent,
      z.infer<typeof socialCommandErrorsSchema>
    >(
      key,
      origin,
      accountId,
      schema,
      commandSchema,
      socialCommandDispositionSchema,
      socialIntentRequestId,
      getStorage,
    ),
    origin,
    accountId,
    subjectId,
    slot,
  };
}

/** Only this fenced atom writes composer text; blank/partial text is a draft, never a send. */
export function createSocialDraft(
  origin: string,
  accountId: string,
  subjectId: string,
  conversationId: string,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  if (publicOrigin(origin) !== origin)
    throw new Error("Draft scope is unavailable.");
  for (const id of [accountId, subjectId, conversationId])
    identifierSchema.parse(id);
  const key = `spellwake.social-draft.v1:${origin}:${accountId}:${conversationId}`;
  const schema = z.strictObject({
    version: z.literal(1),
    apiOrigin: z.literal(origin),
    accountId: z.literal(accountId),
    subjectId: z.literal(subjectId),
    conversationId: z.literal(conversationId),
    text: z.string().refine((text) => Array.from(text).length <= 4000),
  });
  const cell = createDurableCell(key, schema, getStorage);
  const saveAtom = atom(null, async (_get, set, text: string) =>
    withAccountStorageLock(origin, accountId, () => {
      const prior = cell.read();
      if (prior.status === "unavailable") {
        set(cell.refreshAtom, prior);
        throw new Error("Saved draft could not be verified.");
      }
      return set(cell.writeAtom, {
        version: 1,
        apiOrigin: origin,
        accountId,
        subjectId,
        conversationId,
        text,
      });
    }),
  );
  const clearIfTextAtom = atom(null, async (_get, set, expected: string) =>
    withAccountStorageLock(origin, accountId, () => {
      const prior = cell.read();
      if (prior.status !== "saved" || prior.value.text !== expected)
        return false;
      return set(cell.writeAtom, { ...prior.value, text: "" }) !== null;
    }),
  );
  return { key, stateAtom: cell.stateAtom, saveAtom, clearIfTextAtom };
}
