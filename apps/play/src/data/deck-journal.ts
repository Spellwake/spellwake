import { z } from "zod";
import {
  identifierSchema,
  commandAckSchema,
  deckCommandSchema,
  deckCommandErrorsSchema,
  deckCommandDispositionSchema,
  type DeckCommand,
} from "@spellwake/application-contracts";
import { createIntentJournal } from "./intent-journal";

const source = {
  version: z.literal(1),
  apiOrigin: z.string(),
  accountId: identifierSchema,
  command: deckCommandSchema,
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
    errors: deckCommandErrorsSchema,
  }),
]);
export type DeckJournal = z.infer<typeof journalSchema>;
export type DeckJournalSlot =
  { kind: "create" } | { kind: "save" | "settings"; deckId: string };
/** Every transition must run under this key's exclusive Web Lock in the mutation controller. */
export function createDeckJournalStore(
  apiOrigin: string,
  accountId: string,
  slot: DeckJournalSlot,
  getStorage: () => Pick<Storage, "getItem" | "setItem">,
) {
  const key = `spellwake.deck-command.v1:${apiOrigin}:${accountId}:${slot.kind}${slot.kind !== "create" ? `:${slot.deckId}` : ""}`;
  const schema = journalSchema.refine(
    (journal) =>
      journal.apiOrigin === apiOrigin &&
      journal.accountId === accountId &&
      journal.command.kind === slot.kind &&
      (slot.kind === "create" ||
        (journal.command.kind !== "create" &&
          journal.command.command.deckId === slot.deckId)),
  );
  return createIntentJournal<
    DeckCommand,
    z.infer<typeof deckCommandErrorsSchema>
  >(
    key,
    apiOrigin,
    accountId,
    schema,
    deckCommandSchema,
    deckCommandDispositionSchema,
    (command) => command.command.requestId,
    getStorage,
  );
}
